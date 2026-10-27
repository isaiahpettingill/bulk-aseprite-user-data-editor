// All bytes stay in this tab; no file upload or analytics.
let incoming = [];
window.editorOpen = function(directory) {
  const input = document.createElement('input'); input.type='file'; input.multiple=true;
  input.accept='.ase,.aseprite'; if (directory) input.webkitdirectory=true;
  input.onchange = async () => {
    let total=0; let count=0;
    for (const file of input.files) {
      const path=file.webkitRelativePath || file.name;
      if (!/\.(ase|aseprite)$/i.test(path)) continue;
      total += file.size; count++; if(total>256*1024*1024 || count>10000){incoming.push({error:'Folder limit exceeded (256 MiB / 10,000 files)'});break;}
      if (file.size>128*1024*1024) { incoming.push({error:path+': exceeds 128 MiB'}); continue; }
      try {
        incoming.push({name:path, bytes:new Uint8Array(await file.arrayBuffer())});
      } catch (error) {
        incoming.push({error:path+': '+(error.message || 'Could not read file')});
      }
    }
  }; input.click();
};
window.editorPoll=()=>incoming.splice(0);
window.editorDownload=(name,data)=>{
  const url=URL.createObjectURL(new Blob([data])); const a=document.createElement('a');
  a.href=url; a.download=name; a.click(); setTimeout(()=>URL.revokeObjectURL(url),30000);
};
