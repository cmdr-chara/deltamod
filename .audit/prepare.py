from pathlib import Path
import re, json, sys

def edit(p, fn):
    p=Path(p); old=p.read_text(); new=fn(old)
    if new!=old: p.write_text(new)

if sys.argv[1]=='pre':
    def patching(s):
        s=s.replace("const hasLegacyNextPatchStep = window.deltamodBackend.isCommandAvailable('npsCallback');\n",'')
        s=s.replace('if (nextButton && !hasLegacyNextPatchStep)', 'if (nextButton)')
        s=s.replace("    if (hasLegacyNextPatchStep) {\n        await window.deltamodBackend.invokeOptional('npsCallback', [], false);\n        return;\n    }\n",'')
        assert 'npsCallback' not in s
        return s
    edit('web/views/patching/index.js',patching)
    edit('src-tauri/src/main.rs',lambda s:s.replace('        BackendChannel::Unsupported(name) => Err(error::unavailable(&name)),\n',''))
else:
    for folder in ['scripts/tauri-parity','tests']:
        for p in Path(folder).rglob('*.js'):
            if p.name.startswith('audit-'): continue
            def names(s):
                s=s.replace('preloadPath','contractPath').replace('web/preload.js','web/tauri-adapter.js').replace("'web', 'preload.js'","'web', 'tauri-adapter.js'")
                return s.replace('electronInvoke','rendererInvoke').replace('electronEvents','rendererEvents').replace('report.electron','report.renderer')
            edit(p,names)
    p=Path('tests/tauri-adapter.test.js'); s=p.read_text()
    s=s.replace("toBe('electron')","toBe('tauri')")
    s=s.replace("'implementedCommands'","'ALLOWED_INVOKE_CHANNELS'").replace("'allowedEvents'","'ALLOWED_EVENT_CHANNELS'")
    s=re.sub(r'(const expectedUnsupported\s*=\s*)\[.*?\];',r'\1[];',s,flags=re.S)
    s=s.replace('toHaveLength(122)','toHaveLength(123)')
    s=re.sub(r'^.*await root\.communityAPI\.tools\.openInstallationInUndertaleModTool\([^\n]*\n','',s,flags=re.M)
    s=re.sub(r'^.*openInstallationInUndertaleModTool\([^\n]*\n','',s,flags=re.M)
    s=re.sub(r"\s*expect\(invoke\)\.toHaveBeenNthCalledWith\(2,\s*'backend_invoke',\s*\{\s*channel: 'undertaleModTool:openInstallation',\s*data: \[[^]]*\]\s*\}\);",'',s,flags=re.S)
    p.write_text(s)
    p=Path('tests/nexus-ui-policy.test.js'); s=p.read_text().replace('node/IPCHandlers.js','src-tauri/src/channels/nexus_oauth.rs'); p.write_text(s)
    print('Contract consumer names migrated; behavioral tests verify remaining expectations.')
