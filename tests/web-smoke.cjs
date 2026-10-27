const { chromium } = require('playwright');
const fs = require('node:fs');
const assert = require('node:assert/strict');
fs.mkdirSync('test-fixtures', {recursive: true});
const file = Buffer.alloc(144);
file.writeUInt32LE(144, 0); file.writeUInt16LE(0xa5e0, 4);
for (const offset of [6, 8, 10]) file.writeUInt16LE(1, offset);
file.writeUInt16LE(32, 12); file.writeUInt32LE(16, 128); file.writeUInt16LE(0xf1fa, 132);
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
    if (errors.length) throw new Error(errors.join('\n'));
    console.log('Web startup, primary multi-file picker, extension filtering, cancel, duplicate import and keyboard selection passed');
  } catch (error) {
    console.log('Runtime state:', await page.evaluate(() => ({status: window.editorStatus, focus: document.activeElement?.tagName})), errors);
    await page.screenshot({path: 'web-failure.png'});
    throw error;
  } finally { await browser.close(); }
})();
