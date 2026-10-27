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
  const window = {addEventListener() {}};
  const document = {
    createElement(tag) {
      assert.equal(tag, 'input');
      const input = {files: [], click() { this.clicked = true; }, addEventListener(type, fn) { this[`on${type}`] = fn; }};
      inputs.push(input);
      return input;
    },
    getElementById(id) { return id === 'editor' ? canvas : null; }
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../web/files.js'), 'utf8'), {
    window, document, Uint8Array, Blob, URL, setTimeout, console
  });
  return {
    window, inputs, reads,
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
  h.window.editorOpen(false);
  h.window.editorOpen(false);
  assert.equal(h.inputs.length, 1);
  await h.inputs[0].oncancel();
  h.window.editorOpen(false);
  assert.equal(h.inputs.length, 2);
  h.inputs[1].files = [];
  await h.inputs[1].onchange();
  h.window.editorOpen(false);
  assert.equal(h.inputs.length, 3);
});

test('unsafe paths are rejected before arrayBuffer', async () => {
  const h = harness();
  await h.pick([h.file('bad.ase', {relative: '../bad.ase'}), h.file('bad.ase', {relative: 'C:/bad.ase'})], true);
  assert.deepEqual(h.reads, []);
  assert.equal(h.poll().filter(f => /unsafe/.test(f.error)).length, 2);
});
