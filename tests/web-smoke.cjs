const { chromium } = require('playwright');
const fs = require('node:fs');
const assert = require('node:assert/strict');
fs.mkdirSync('test-fixtures', {recursive: true});
const initialText = Buffer.from('credits=Original');
const palette = Buffer.alloc(8); palette.writeUInt32LE(8); palette.writeUInt16LE(4, 4);
const userData = Buffer.alloc(12 + initialText.length);
userData.writeUInt32LE(userData.length); userData.writeUInt16LE(0x2020, 4);
userData.writeUInt32LE(1, 6); userData.writeUInt16LE(initialText.length, 10); initialText.copy(userData, 12);
const file = Buffer.alloc(144 + palette.length + userData.length);
file.writeUInt32LE(file.length, 0); file.writeUInt16LE(0xa5e0, 4);
for (const offset of [6, 8, 10]) file.writeUInt16LE(1, offset);
file.writeUInt16LE(32, 12); file.writeUInt32LE(file.length - 128, 128); file.writeUInt16LE(0xf1fa, 132);
file.writeUInt16LE(2, 134); palette.copy(file, 144); userData.copy(file, 144 + palette.length);

// Inspect the generated ZIP and Aseprite chunk bytes independently of the app.
function exportedTexts(archive) {
  const bytes = Buffer.from(archive);
  let end = bytes.length - 22;
  while (end >= Math.max(0, bytes.length - 65557) && bytes.readUInt32LE(end) !== 0x06054b50) end--;
  assert.ok(end >= 0, 'ZIP end record missing');
  const result = {};
  let cursor = bytes.readUInt32LE(end + 16);
  for (let entry = 0; entry < bytes.readUInt16LE(end + 10); entry++) {
    assert.equal(bytes.readUInt32LE(cursor), 0x02014b50);
    const method = bytes.readUInt16LE(cursor + 10);
    const size = bytes.readUInt32LE(cursor + 20);
    const nameLength = bytes.readUInt16LE(cursor + 28);
    const name = bytes.subarray(cursor + 46, cursor + 46 + nameLength).toString();
    const local = bytes.readUInt32LE(cursor + 42);
    assert.equal(bytes.readUInt32LE(local), 0x04034b50);
    const payload = local + 30 + bytes.readUInt16LE(local + 26) + bytes.readUInt16LE(local + 28);
    let sprite = bytes.subarray(payload, payload + size);
    if (method === 8) sprite = require('node:zlib').inflateRawSync(sprite);
    else assert.equal(method, 0, 'Unexpected ZIP compression');
    assert.equal(sprite.readUInt16LE(4), 0xa5e0);
    const texts = [];
    let chunk = 144;
    while (chunk < sprite.length) {
      const length = sprite.readUInt32LE(chunk);
      assert.ok(length >= 6 && chunk + length <= sprite.length);
      if (sprite.readUInt16LE(chunk + 4) === 0x2020 && (sprite.readUInt32LE(chunk + 6) & 1)) {
        const textLength = sprite.readUInt16LE(chunk + 10);
        texts.push(sprite.subarray(chunk + 12, chunk + 12 + textLength).toString());
      }
      chunk += length;
    }
    assert.equal(texts.length, 1, 'Fixture must retain one sprite user-data field');
    result[name] = texts[0];
    cursor += 46 + nameLength + bytes.readUInt16LE(cursor + 30) + bytes.readUInt16LE(cursor + 32);
  }
  return result;
}
for (const name of ['one.aseprite', 'two.ASE', 'three.aseprite', 'unsupported.png']) fs.writeFileSync(`test-fixtures/${name}`, file);
(async () => {
  const browser = await chromium.launch({headless: true, args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader']});
  const page = await browser.newPage({viewport: {width: 1100, height: 750}});
  await page.addInitScript(() => {
    window.readFileNames = [];
    const read = File.prototype.arrayBuffer;
    File.prototype.arrayBuffer = function(...args) {
      window.readFileNames.push(this.name);
      return read.apply(this, args);
    };
  });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', msg => { if (msg.type() === 'error') errors.push(msg.text()); });
  try {
    await page.goto('http://127.0.0.1:8774');
    await page.waitForFunction(() => window.editorStatus?.ready === true);
    await page.evaluate(() => {
      window.importedFileNames = [];
      window.exportedArchives = [];
      const download = window.editorDownload;
      window.editorDownload = (name, data) => {
        window.exportedArchives.push(Array.from(data));
        download(name, data);
      };
      const poll = window.editorPoll;
      window.editorPoll = () => {
        const batch = poll();
        window.importedFileNames.push(...batch.filter(item => item.name).map(item => item.name));
        return batch;
      };
    });
    await page.waitForTimeout(500);
    await page.screenshot({path: 'web-empty.png'});
    const chooser = page.waitForEvent('filechooser');
    // Open files is the primary (first) toolbar action, independent of folder support.
    await page.mouse.click(40, 15);
    const filesPicker = await chooser;
    assert.equal(filesPicker.isMultiple(), true);
    assert.deepEqual(await filesPicker.element().evaluate(input => ({
      type: input.type, multiple: input.multiple, accept: input.accept, directory: input.webkitdirectory
    })), {type: 'file', multiple: true, accept: '.ase,.aseprite', directory: false});
    await filesPicker.setFiles(['test-fixtures/one.aseprite', 'test-fixtures/two.ASE', 'test-fixtures/three.aseprite', 'test-fixtures/unsupported.png']);
    await page.waitForFunction(() => window.editorStatus?.files === 3);
    assert.deepEqual(await page.evaluate(() => window.readFileNames), ['one.aseprite', 'two.ASE', 'three.aseprite']);
    assert.deepEqual(await page.evaluate(() => window.importedFileNames), ['one.aseprite', 'two.ASE', 'three.aseprite']);
    console.log('Imported three supported files without reading the unsupported file');
    const cancelChooser = page.waitForEvent('filechooser');
    await page.mouse.click(40, 15);
    const cancelled = await cancelChooser;
    await cancelled.setFiles([]);
    await cancelled.element().evaluate(input => input.dispatchEvent(new Event('cancel')));
    await page.waitForTimeout(250);
    assert.equal(await page.evaluate(() => window.editorStatus.files), 3, 'Cancel must not replace existing imports');
    const duplicateChooser = page.waitForEvent('filechooser');
    await page.mouse.click(40, 15);
    await (await duplicateChooser).setFiles(['test-fixtures/one.aseprite']);
    await page.waitForFunction(() => window.editorStatus?.files === 4);
    assert.deepEqual(await page.evaluate(() => window.importedFileNames), ['one.aseprite', 'two.ASE', 'three.aseprite', 'one (2).aseprite']);
    await page.mouse.click(70, 73);
    await page.waitForTimeout(200);
    await page.keyboard.press('Control+a');
    await page.waitForFunction(() => window.editorStatus?.selected === 4);
    await page.keyboard.press('ArrowDown');
    await page.waitForFunction(() => window.editorStatus?.selected === 1);
    await page.keyboard.press('Control+ArrowDown');
    await page.waitForTimeout(300);
    if (await page.evaluate(() => window.editorStatus.selected) !== 1) throw new Error('Ctrl+arrow unexpectedly changed selection');
    await page.screenshot({path: 'web-loaded.png'});
    // Canvas controls apply focus and text events on animation frames, unlike DOM inputs.
    const renderInput = () => page.evaluate(() => new Promise(resolve => {
      requestAnimationFrame(() => requestAnimationFrame(resolve));
    }));
    const clickControl = async name => {
      await page.waitForFunction(key => Array.isArray(window.editorUi?.[key]), name);
      const [x, y] = await page.evaluate(key => window.editorUi[key], name);
      await page.mouse.click(x, y);
      await renderInput();
      if (['text', 'find', 'replace'].includes(name)) {
        await page.waitForFunction(key => window.editorUi?.[key + 'Focused'] === true, name);
      }
    };
    const saveArchive = async () => {
      const download = page.waitForEvent('download');
      await page.keyboard.press('Control+s');
      const artifact = await download;
      assert.equal(artifact.suggestedFilename(), 'edited-sprites.zip');
      return exportedTexts(fs.readFileSync(await artifact.path()));
    };
    const assertAppendOnce = texts => {
      assert.equal(Object.keys(texts).length, 4);
      for (const [name, text] of Object.entries(texts)) {
        assert.equal(text.split('license=CC0').length - 1, 1, `${name} must receive the append exactly once`);
      }
    };
    // A single document starts with its existing value and saves directly from text focus.
    await clickControl('file-0');
    await page.waitForFunction(() => window.editorStatus?.selected === 1);
    await clickControl('text');
    await page.keyboard.press('Control+a');
    await page.keyboard.type('credits=Tester');
    await renderInput();
    const single = await saveArchive();
    assert.deepEqual(single, {'one.aseprite': 'credits=Tester'});
    // Bulk append uses the same Ctrl+S path, with no mandatory preview/apply click.
    await clickControl('file-0');
    await page.keyboard.press('Control+a');
    await page.waitForFunction(() => window.editorStatus?.selected === 4);
    await clickControl('text');
    await page.keyboard.press('Control+a');
    await page.keyboard.type('license=CC0');
    await renderInput();
    const bulk = await saveArchive();
    assertAppendOnce(bulk);
    assert.equal(bulk['one.aseprite'], 'credits=Tester\nlicense=CC0');
    // A second save may be a no-op or export again, but never re-applies the append draft.
    await page.keyboard.press('Control+s');
    await page.waitForTimeout(350);
    const latest = await page.evaluate(() => window.exportedArchives.at(-1));
    assert.deepEqual(exportedTexts(latest), bulk);
    // Both editor shortcuts open/focus the selected-files find tab rather than browser search.
    await page.keyboard.press('Control+h');
    await page.waitForFunction(() => window.editorUi?.findTab && window.editorUi?.findFocused);
    await page.keyboard.press('Control+a');
    await page.keyboard.type('Original');
    await renderInput();
    await page.waitForFunction(() => window.editorUi?.matches === 3);
    await clickControl('replace');
    await page.keyboard.type('Updated');
    await renderInput();
    await clickControl('replace_all');
    await page.waitForFunction(() => window.editorUi?.matches === 0);
    const replaced = await saveArchive();
    assertAppendOnce(replaced);
    assert.equal(replaced['one.aseprite'], 'credits=Tester\nlicense=CC0');
    for (const [name, text] of Object.entries(replaced)) {
      if (name !== 'one.aseprite') assert.equal(text, 'credits=Updated\nlicense=CC0');
    }
    await page.keyboard.press('Control+f');
    await page.waitForFunction(() => window.editorUi?.findTab && window.editorUi?.findFocused);
    await page.keyboard.press('Control+a');
    await page.keyboard.type('Tester');
    await renderInput();
    await page.waitForFunction(() => window.editorUi?.matches === 1);
    await page.screenshot({path: 'web-find-replace.png'});
    console.log('Direct Ctrl+S export, single edit, append idempotency and selected-files Ctrl+F/H replacement passed');
    if (errors.length) throw new Error(errors.join('\n'));
    console.log('Web startup, primary multi-file picker, extension filtering, cancel, duplicate import and keyboard selection passed');
  } catch (error) {
    console.log('Runtime state:', await page.evaluate(() => ({status: window.editorStatus, focus: document.activeElement?.tagName})), errors);
    await page.screenshot({path: 'web-failure.png'});
    throw error;
  } finally { await browser.close(); }
})();
