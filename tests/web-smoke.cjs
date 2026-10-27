const { chromium } = require('playwright');
const fs = require('node:fs');
fs.mkdirSync('test-fixtures', {recursive: true});
const file = Buffer.alloc(144);
file.writeUInt32LE(144, 0); file.writeUInt16LE(0xa5e0, 4);
for (const offset of [6, 8, 10]) file.writeUInt16LE(1, offset);
file.writeUInt16LE(32, 12); file.writeUInt32LE(16, 128); file.writeUInt16LE(0xf1fa, 132);
for (const name of ['one', 'two', 'three']) fs.writeFileSync(`test-fixtures/${name}.aseprite`, file);
(async () => {
  const browser = await chromium.launch({headless: true, args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader']});
  const page = await browser.newPage({viewport: {width: 1100, height: 750}});
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', msg => { if (msg.type() === 'error') errors.push(msg.text()); });
  try {
    await page.goto('http://127.0.0.1:8774');
    await page.waitForFunction(() => window.editorStatus?.ready === true);
    await page.waitForTimeout(500);
    await page.screenshot({path: 'web-empty.png'});
    const chooser = page.waitForEvent('filechooser');
    await page.mouse.click(137, 15);
    await (await chooser).setFiles(['test-fixtures/one.aseprite', 'test-fixtures/two.aseprite', 'test-fixtures/three.aseprite']);
    await page.waitForFunction(() => window.editorStatus?.files === 3);
    console.log('Imported three files', await page.evaluate(() => window.editorStatus));
    await page.mouse.click(70, 73);
    await page.waitForTimeout(200);
    await page.keyboard.press('Control+a');
    await page.waitForFunction(() => window.editorStatus?.selected === 3);
    await page.keyboard.press('ArrowDown');
    await page.waitForFunction(() => window.editorStatus?.selected === 1);
    await page.keyboard.press('Control+ArrowDown');
    await page.waitForTimeout(300);
    if (await page.evaluate(() => window.editorStatus.selected) !== 1) throw new Error('Ctrl+arrow unexpectedly changed selection');
    await page.screenshot({path: 'web-loaded.png'});
    if (errors.length) throw new Error(errors.join('\n'));
    console.log('Web startup, file import and keyboard selection passed');
  } catch (error) {
    console.log('Runtime state:', await page.evaluate(() => ({status: window.editorStatus, focus: document.activeElement?.tagName})), errors);
    await page.screenshot({path: 'web-failure.png'});
    throw error;
  } finally { await browser.close(); }
})();
