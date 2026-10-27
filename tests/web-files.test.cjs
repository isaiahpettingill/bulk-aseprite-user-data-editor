const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function harness() {
  const inputs = [];
  const reads = [];
  let focused = 0;
  const canvas = {focus() { focused++; }};
  const listeners = {};
  const window = {addEventListener(type, listener) { listeners[type] = listener; }};
  const document = {
    body: {appendChild(input) { input.connected = true; }},
    createElement(tag) {
      assert.equal(tag, 'input');
      const input = {files: [], remove() { this.connected = false; }, click() { this.clicked = true; }, addEventListener(type, fn) { this[`on${type}`] = fn; }};
      inputs.push(input);
      return input;
    },
    getElementById(id) { return id === 'editor' ? canvas : null; }
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../web/files.js'), 'utf8'), {
    window, document, navigator: {}, Uint8Array, Blob, URL, setTimeout, console
  });
  return {
    window, inputs, reads, listeners,
    focused: () => focused,
    file(name, {relative = '', bytes = [1, 2, 3], size = bytes.length, error} = {}) {
      return {name, webkitRelativePath: relative, size, async arrayBuffer() {
        reads.push(name);
        if (error) throw new Error(error);
        return Uint8Array.from(bytes).buffer;
      }};
    },
    async pick(files, directory = false) {
      window.editorOpen(directory);
      const input = inputs.at(-1);
      input.files = files;
      await input.onchange();
      return input;
    },
    poll() { return Array.from(window.editorPoll()); }
  };
}

test('Open files creates a multi-file Aseprite picker, not a directory picker', () => {
  const h = harness();
  h.window.editorOpen(false);
  const input = h.inputs[0];
  assert.equal(input.type, 'file');
  assert.equal(input.multiple, true);
  assert.equal(input.accept, '.ase,.aseprite');
  assert.equal(Boolean(input.webkitdirectory), false);
  assert.equal(input.clicked, true);
  assert.equal(input.connected, true);
});

test('extension filtering is case-insensitive and occurs before reading bytes', async () => {
  const h = harness();
  await h.pick([h.file('tile.ASE'), h.file('sprite.ASEPRITE'), h.file('image.png'), h.file('sprite.aseprite.png'), h.file('notes.txt')]);
  assert.deepEqual(h.reads, ['tile.ASE', 'sprite.ASEPRITE']);
  assert.deepEqual(h.poll().filter(f => f.name).map(f => f.name), ['tile.ASE', 'sprite.ASEPRITE']);
  assert.ok(h.focused() > 0);
});

test('cancelling or an empty selection leaves pending imports unchanged', async () => {
  const h = harness();
  await h.pick([h.file('keep.aseprite')]);
  h.window.editorOpen(false);
  const input = h.inputs.at(-1);
  // Cancelling the platform picker does not invoke change; browsers may emit cancel.
  if (input.oncancel) await input.oncancel();
  await h.pick([]);
  assert.deepEqual(h.poll().filter(f => f.name).map(f => f.name), ['keep.aseprite']);
  assert.equal(h.poll().length, 0);
});

test('repeated basenames receive stable suffixes instead of replacing an import', async () => {
  const h = harness();
  await h.pick([h.file('one.aseprite')]);
  assert.deepEqual(h.poll().filter(f => f.name).map(f => f.name), ['one.aseprite']);
  await h.pick([h.file('one.aseprite'), h.file('one.aseprite')]);
  assert.deepEqual(h.poll().filter(f => f.name).map(f => f.name), ['one (2).aseprite', 'one (3).aseprite']);
});

test('explicit names cannot collide with generated duplicate names', async () => {
  const h = harness();
  await h.pick([h.file('one.aseprite'), h.file('one (2).aseprite'), h.file('one.aseprite')]);
  const names = h.poll().filter(f => f.name).map(f => f.name);
  assert.equal(new Set(names).size, 3);
  assert.deepEqual(names, ['one.aseprite', 'one (2).aseprite', 'one (3).aseprite']);
});

test('folder picker preserves relative paths and separates same-named files', async () => {
  const h = harness();
  const input = await h.pick([
    h.file('tile.aseprite', {relative: 'sprites/a/tile.aseprite'}),
    h.file('tile.aseprite', {relative: 'sprites/b/tile.aseprite'})
  ], true);
  assert.equal(input.webkitdirectory, true);
  assert.deepEqual(h.poll().filter(f => f.name).map(f => f.name), ['sprites/a/tile.aseprite', 'sprites/b/tile.aseprite']);
});

test('a read failure is visible and does not prevent other selected files loading', async () => {
  const h = harness();
  await h.pick([h.file('bad.ase', {error: 'read denied'}), h.file('good.ase')]);
  const output = h.poll();
  assert.match(output[0].error, /bad\.ase.*read denied/);
  assert.equal(output[1].name, 'good.ase');
});

test('oversized files are rejected before allocating their data buffer', async () => {
  const h = harness();
  await h.pick([h.file('large.ase', {size: 129 * 1024 * 1024})]);
  assert.deepEqual(h.reads, []);
  assert.match(h.poll()[0].error, /128 MiB/);
});

test('collisions are case-insensitive', async () => {
  const h = harness();
  await h.pick([h.file('Tile.ASE')]); h.poll();
  await h.pick([h.file('tile.ase')]);
  assert.deepEqual(h.poll().filter(f => f.name).map(f => f.name), ['tile (2).ase']);
});

test('overlapping picker calls are ignored until cancellation releases the picker', async () => {
  const h = harness();
  assert.equal(h.window.editorPickerBusy(), false);
  h.window.editorOpen(false);
  assert.equal(h.window.editorPickerBusy(), true);
  h.window.editorOpen(false);
  assert.equal(h.inputs.length, 1);
  await h.inputs[0].oncancel();
  assert.equal(h.window.editorPickerBusy(), false);
  h.window.editorOpen(false);
  assert.equal(h.inputs.length, 2);
  h.inputs[1].files = [];
  await h.inputs[1].onchange();
  assert.equal(h.window.editorPickerBusy(), false);
  h.window.editorOpen(false);
  assert.equal(h.inputs.length, 3);
});

test('unsafe paths are rejected before arrayBuffer', async () => {
  const h = harness();
  await h.pick([h.file('bad.ase', {relative: '../bad.ase'}), h.file('bad.ase', {relative: 'C:/bad.ase'})], true);
  assert.deepEqual(h.reads, []);
  assert.equal(h.poll().filter(f => /unsafe/.test(f.error)).length, 2);
});


test('editor shortcuts prevent browser defaults without stealing ordinary text/select-all', () => {
  const h = harness();
  for (const key of ['s', 'S', 'f', 'F', 'h', 'H']) {
    for (const modifier of ['ctrlKey', 'metaKey']) {
      let prevented = false;
      h.listeners.keydown({key, [modifier]: true, preventDefault() { prevented = true; }});
      assert.equal(prevented, true, `${modifier}+${key} should use the editor shortcut`);
    }
  }
  for (const event of [{key: 'f'}, {key: 'h'}, {key: 'a', ctrlKey: true}, {key: 'a', metaKey: true}]) {
    let prevented = false;
    h.listeners.keydown({...event, preventDefault() { prevented = true; }});
    assert.equal(prevented, false, `Do not intercept ${JSON.stringify(event)}`);
  }
});


test('picker stays busy during asynchronous file reading and releases after completion', async () => {
  const h = harness();
  let complete;
  const pending = h.file('wait.ase');
  pending.arrayBuffer = () => new Promise(resolve => { complete = resolve; });
  h.window.editorOpen(false);
  h.inputs[0].files = [pending];
  const loading = h.inputs[0].onchange();
  assert.equal(h.window.editorPickerBusy(), true);
  h.window.editorOpen(false);
  assert.equal(h.inputs.length, 1);
  complete(Uint8Array.from([1, 2, 3]).buffer);
  await loading;
  assert.equal(h.window.editorPickerBusy(), false);
  assert.equal(h.focused(), 1);
  assert.equal(h.poll()[0].name, 'wait.ase');
});
