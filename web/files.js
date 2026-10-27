// All bytes stay in this tab; no file upload or analytics.
let incoming = [];
const importedNames = new Set();
let pickerOpen = false;
function availableName(path) {
  const slash = path.lastIndexOf('/');
  const dot = path.lastIndexOf('.');
  const stem = dot > slash ? path.slice(0, dot) : path;
  const extension = dot > slash ? path.slice(dot) : '';
  let candidate = path;
  for (let n = 2; importedNames.has(candidate.toLowerCase()); n++) {
    candidate = `${stem} (${n})${extension}`;
  }
  return candidate;
}
window.editorOpen = function(directory) {
  if (pickerOpen) return;
  const input = document.createElement('input');
  input.type = 'file';
  input.multiple = true;
  input.accept = '.ase,.aseprite';
  if (directory) input.webkitdirectory = true;
  // Imports are additive: existing documents and unsaved edits are never replaced.
  pickerOpen = true;
  const finish = () => {
    pickerOpen = false;
    document.getElementById('editor')?.focus();
  };
  input.oncancel = finish;
  input.onchange = async () => {
    let total = 0, count = 0, skipped = 0, renamed = 0;
    try {
      for (const file of input.files) {
        const path = file.webkitRelativePath || file.name;
        // Validate before arrayBuffer: unrelated files never enter WASM memory.
        if (!/\.(ase|aseprite)$/i.test(path)) { skipped++; continue; }
        if (path.split(/[\\/]/).some(part => part === '..' || part === '' || part === '.') || path.includes(':')) {
          incoming.push({error: 'Skipped unsafe file path'}); continue;
        }
        if (file.size > 128 * 1024 * 1024) {
          incoming.push({error: path + ': exceeds 128 MiB'}); continue;
        }
        total += file.size;
        if (total > 256 * 1024 * 1024 || count >= 10000) {
          incoming.push({error: 'Selection limit exceeded (256 MiB / 10,000 files)'}); break;
        }
        try {
          const bytes = new Uint8Array(await file.arrayBuffer());
          const name = availableName(path);
          importedNames.add(name.toLowerCase());
          if (name !== path) renamed++;
          incoming.push({name, bytes});
          count++;
        } catch (error) {
          incoming.push({error: path + ': ' + (error.message || 'Could not read file')});
        }
      }
      if (skipped || renamed) {
        incoming.push({status: [skipped ? `${skipped} unsupported skipped` : '', renamed ? `${renamed} renamed to avoid duplicates` : ''].filter(Boolean).join(' · ')});
      }
    } finally { finish(); }
  };
  try { input.click(); } catch (error) { finish(); throw error; }
};
window.editorPoll=()=>incoming.splice(0);
window.editorDownload=(name,data)=>{
  const url=URL.createObjectURL(new Blob([data])); const a=document.createElement('a');
  a.href=url; a.download=name; a.click(); setTimeout(()=>URL.revokeObjectURL(url),30000);
};
let hasUnsavedEdits=false;
window.editorDirty=(dirty)=>{hasUnsavedEdits=dirty;};
window.addEventListener('beforeunload',event=>{if(hasUnsavedEdits){event.preventDefault();event.returnValue='';}});
window.addEventListener('keydown',event=>{if((event.ctrlKey||event.metaKey)&&['s','f','h'].includes(event.key.toLowerCase()))event.preventDefault();});

window.editorStatus={files:0,selected:0,ready:false};
window.editorReport=(files,selected)=>{window.editorStatus={files,selected,ready:true};};
