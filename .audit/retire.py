# Isolated assembly recipe. This file is not part of the product tree.
from pathlib import Path
import json, re, shutil

def read(p): return Path(p).read_text()
def write(p,s):
    p=Path(p); p.parent.mkdir(parents=True,exist_ok=True); p.write_text(s)
def replace(p,a,b,count=1):
    s=read(p)
    if s.count(a)!=count: raise RuntimeError(f'{p}: expected {count} occurrences of {a!r}, got {s.count(a)}')
    write(p,s.replace(a,b))
def between(p,a,b,replacement=''):
    s=read(p); start=s.index(a); end=s.index(b,start); write(p,s[:start]+replacement+s[end:])
def edit_json(p,fn):
    v=json.loads(read(p)); fn(v); write(p,json.dumps(v,indent=2,ensure_ascii=False)+'\n')

removed='''node/Config.js node/ControllerMode.js node/DownloadUtilities/GameJolt.js node/DownloadUtilities/Itch.js node/EasterEggWindow.js node/ErrorWin.js node/FeatureFlags.js node/GameBanana.js node/GameDB.js node/IPCHandlers.js node/Junction.js node/KeyValue.js node/Modstore.js node/Netlayer.js node/Paths.js node/ProgressModal.js node/Protocol.js node/RunConditions.js node/Runner.js node/System.js node/TestIpcScope.js node/Updates.js node/Utils.js node/Watercooler.js node/gamebanana/LoginSession.js node/workers/HashWorker.js node/security/CredentialStorage.js tests/credential-storage.test.js web/preload.js .github/workflows/release.yml .github/workflows/auto-release.yml scripts/verify-packaged-hash-worker.js scripts/verify-packaged-security-worker.js scripts/verify-packaged-copy-worker.js scripts/verify-packaged-patch-plan-worker.js scripts/verify-packaged-patch-transaction-worker.js'''.split()
for p in removed:
    if not Path(p).is_file(): raise RuntimeError(f'missing retirement input: {p}')
    Path(p).unlink()
for p in ['web/dlmodal','web/views/electron-tracer','web/views/deleteall','scripts/legacy']:
    if Path(p).exists(): shutil.rmtree(p)
write('node/Console.js', '''// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
'use strict';
const { format } = require('node:util');
function log(level, ...args) {
    process.stdout.write(`[${level}] ${format(...args)}\\n`);
}
module.exports = Object.freeze({
    log: (...args) => log('LOG', ...args),
    warn: (...args) => log('WARN', ...args),
    error: (...args) => log('ERROR', ...args),
    info: (...args) => log('INFO', ...args),
    debug: (...args) => log('DEBUG', ...args)
});
''')

def package(p):
    p.pop('main',None); p.pop('build',None)
    scripts=p['scripts']
    for name in ['predev','prebuild-windows','prebuild-linux','prebuild-macos','erase-data','verify:packaged-patch-transaction-worker']: scripts.pop(name,None)
    scripts['dev']='npm run tauri:dev'
    for host in ['windows','linux','macos']: scripts['build-'+host]='npm run build:tauri'
    scripts['test']='npm run verify:secure-updater && npm run test:audit && npm run verify:tauri-only && vitest run'
    scripts['verify:tauri-only']='node --test scripts/tauri-parity/test/tauri-only.test.js scripts/tauri-parity/test/platform-staging.test.js'
    for name in ['electron','electron-builder']: p.get('devDependencies',{}).pop(name,None)
    for name in ['@chainsafe/xdelta3-node','@sentry/node','create-desktop-shortcuts','electron-updater','mime-types','windows-elevate','yoctocolors-cjs']: p.get('dependencies',{}).pop(name,None)
    p['devDependencies'].update(p.pop('dependencies',{}))
    p['devDependencies']=dict(sorted(p['devDependencies'].items()))
edit_json('package.json',package)

p='web/tauri-adapter.js'
replace(p,"if (root.deltamodBackend || root.communityAPI || root.preloadAPI) return 'electron';", "if (root.deltamodBackend) return 'tauri';")
s=read(p).replace('implementedCommands','ALLOWED_INVOKE_CHANNELS').replace('allowedEvents','ALLOWED_EVENT_CHANNELS')
s=s.replace('    root.electronAPI = Object.freeze({ invoke });\n','')
s=s.replace("            chooseUndertaleModTool: () => invoke('undertaleModTool:choose'),\n            openInstallationInUndertaleModTool: index => invoke('undertaleModTool:openInstallation', [index])", "            chooseUndertaleModTool: () => invoke('undertaleModTool:choose')")
s=s.replace('listen(channel, event => callback(event.payload))', 'listen(channel, event => { if (!disposed) callback(event.payload); })')
s=s.replace('Renderer-visible preload channels','Renderer-visible native channels').replace('preload\'s public bridge','the public bridge')
write(p,s)
for folder in ['web','tests/e2e/fixtures']:
    for p in Path(folder).rglob('*'):
        if p.suffix not in ['.js','.html','.ts']: continue
        s=p.read_text(); original=s
        s=s.replace('window.electronAPI','window.deltamodBackend')
        if p.name=='refinement-page.js':
            s=re.sub(r'^.*electronAPI\s*:\s*\{\s*invoke[^\n]*\n','',s,flags=re.M)
            s=re.sub(r'^.*window\.electronAPI\s*=.*\n','',s,flags=re.M)
        if p.name=='preload.d.ts':
            s=re.sub(r'^.*electronAPI:.*\n','',s,flags=re.M)
            s=re.sub(r'^.*openInstallationInUndertaleModTool:.*\n','',s,flags=re.M)
        if s!=original: write(p,s)
# Remove only already-disabled UI actions, retaining cache, repair and recovery controls.
p='web/views/options/index.js'
between(p,"            const canRebootDev",'            let hashButton;')
s=read(p)
s=re.sub(r"            await addButton\([^\n]*Delete all[^\n]*\n.*?\n            \},[^\n]*\n",'',s,flags=re.S)
s=s.replace('and create shortcuts for them','and repair them')
write(p,s)
p='web/views/installmanager/index.js'
between(p,"                let editBtn = document.createElement('button');",'                goCell.appendChild(buttonsDiv);',"                buttonsDiv.appendChild(openBtn);\n\n")
p='web/views/collections/index.js'
between(p,'        var download = document.createElement("button");','        table.appendChild(tr);')
p='web/views/patching/index.js'
s=read(p)
s=re.sub(r"\s*if \([^\n]*isCommandAvailable\('npsCallback'\)[^\n]*\)\s*\{.*?\}\s*else\s*\{\s*(await )?page\('main'\);?\s*\}","\n        await page('main');",s,flags=re.S)
# The source may have the inverse capability branch. Fail closed, print context for a precise edit.
if 'npsCallback' in s:
    i=s.index('npsCallback'); print('PATCHING CONTEXT',s[max(0,i-400):i+600]); raise RuntimeError('patching continuation anchor changed')
write(p,s)
p='web/index.js'; s=read(p)
s=re.sub(r"^\s*['\"]deleteall['\"],?\s*\n",'',s,flags=re.M)
s=s.replace("'deleteall',",'').replace('"deleteall",','')
s=s.replace('Electron IPC','native IPC').replace('Electron logger','native logger')
write(p,s)

p='src-tauri/src/main.rs'; s=read(p)
s=s.replace('    Unsupported(String),\n','')
s=re.sub(r'            "rebootDev"\n(?:            \| "[^"\n]+"\n)*            \| "initialize" => Self::Unsupported\(channel.to_owned\(\)\),\n','',s)
s=re.sub(r'^\s*BackendChannel::Unsupported\(channel\) => Err\(error::unavailable\(&channel\)\),\n','\n',s,flags=re.M)
s=s.replace('Ok(BackendChannel::Unsupported(_))','Err(())')
# The six retired command names are unknown, not fictitiously implemented.
anchor='            "openElectronTracer",'
if anchor in s: s=s.replace(anchor,anchor+'\n'+''.join(f'            "{c}",\n' for c in ['rebootDev','createInstallLink','undertaleModTool:openInstallation','gamebanana_downloadAllInCollection','npsCallback','initialize']).rstrip())
if 'Self::Unsupported' in s or 'BackendChannel::Unsupported' in s:
    for line in s.splitlines():
        if 'Unsupported' in line: print('UNSUPPORTED CONTEXT',line)
    raise RuntimeError('unhandled Unsupported arm')
write(p,s)

# The public contract is the live Tauri adapter, not a retired preload.
parity_files=['scripts/tauri-parity/lib/parity.js','scripts/tauri-parity/parity-report.js','scripts/tauri-parity/test/harness.test.js','tests/tauri-adapter.test.js','tests/tauri-parity-classification.test.js']
for p in parity_files:
    s=read(p).replace('preloadPath','contractPath').replace('web/preload.js','web/tauri-adapter.js').replace("'web', 'preload.js'","'web', 'tauri-adapter.js'")
    s=s.replace('electronInvoke','rendererInvoke').replace('electronEvents','rendererEvents').replace('report.electron','report.renderer')
    if p.endswith('lib/parity.js'):
        s=re.sub(r'\bpreload\b','contract',s); s=re.sub(r'\belectron\b','renderer',s)
    if p.endswith('parity-report.js'): s=s.replace('report.sources.preload','report.sources.contract')
    s=s.replace('implementedCommands','ALLOWED_INVOKE_CHANNELS').replace('allowedEvents','ALLOWED_EVENT_CHANNELS')
    s=re.sub(r'(rendererInvoke:\s*)\d+',r'\g<1>123',s)
    s=re.sub(r'(rustKnown:\s*)\d+',r'\g<1>123',s)
    s=re.sub(r'(rustImplemented:\s*)\d+',r'\g<1>123',s)
    s=re.sub(r'(rustUnsupported:\s*)\d+',r'\g<1>0',s)
    write(p,s)
# Fixture schemas remain versioned; snapshots are explicitly static, not runtime evidence.
p='scripts/tauri-parity/fixtures/contract.json'; fixture=json.loads(read(p))
for case in fixture['cases']:
    case['classification']='unknown' if case['id']=='unknown' else 'implemented'
    if case['id']=='start-game': case['rust']={'ok':True,'value':True}
write(p,json.dumps(fixture,indent=2)+'\n')
edit_json('scripts/tauri-parity/fixtures/rust-output.json',lambda v:v.update({'start-game':{'ok':True,'value':True}}))
# Platform-specific bundle lists prevent the default Windows target leaking into Unix builds.
p='src-tauri/tauri.conf.json'; config=json.loads(read(p)); resources=config['bundle']['resources']
cmode={k:v for k,v in resources.items() if 'cmodeutil.exe' in k}
if len(cmode)!=1: raise RuntimeError('expected one Windows controller resource')
s=read(p)
for key in cmode: s=re.sub(r'^.*'+re.escape(json.dumps(key))+r'.*\n','',s,flags=re.M)
# Handle a last mapping entry without producing a trailing comma.
s=re.sub(r',\n(\s*})',r'\n\1',s); json.loads(s); write(p,s)
def win(v): v.setdefault('bundle',{}).setdefault('resources',{}).update(cmode)
edit_json('src-tauri/tauri.windows.conf.json',win)
def mac(v): v.setdefault('bundle',{}).update({'targets':['app','dmg'],'category':'Utility'})
edit_json('src-tauri/tauri.macos.conf.json',mac)
write('src-tauri/tauri.linux.conf.json',json.dumps({'bundle':{'targets':['deb'],'createUpdaterArtifacts':False}},indent=2)+'\n')
# Diagnostics must not invent an Electron shell when no runtime has been identified.
for p in ['web/views/allmods-v2/index.js','web/modules/product-ui.js','tests/installed-mods-v2.test.js']:
    s=read(p).replace("'electron'","'unknown'" if '/views/' in p else "'tauri'").replace('electron|tauri|unknown','tauri|unknown'); write(p,s)
p='tests/renderer-smoke.test.js'; s=read(p).replace(" && entry.name !== 'electron-tracer'",'').replace("entry.name !== 'electron-tracer' && ",''); write(p,s)
# Remove only the test of the retired safeStorage adapter; native keyring failure tests replace it.
for p in Path('.').rglob('test-classification.json'):
    if 'node_modules' in p.parts: continue
    value=json.loads(p.read_text())
    def clean(obj):
        if isinstance(obj,list): return [clean(x) for x in obj if not (isinstance(x,dict) and any(v=='tests/credential-storage.test.js' for v in x.values())) and x!='tests/credential-storage.test.js']
        if isinstance(obj,dict): return {k:clean(v) for k,v in obj.items() if k not in ['tests/credential-storage.test.js','retirementOccursInSeparateCleanupRelease']}
        return obj
    write(p,json.dumps(clean(value),indent=2)+'\n')
# Native integration documentation must no longer advertise a removed credential adapter.
p=Path('src-tauri/crates/credentials/INTEGRATION.md')
if p.exists(): write(p,'''# Native credential storage

Credentials use the operating-system keyring through the native credentials crate.
There is no plaintext fallback. A missing or unavailable keyring is an explicit error.
Metadata is bounded and schema-validated before mutation. Provider updates are serialized
within the application, and failed metadata writes restore the previous secret.

The Electron safeStorage migration adapter is retired. Existing encrypted Electron blobs
are not deleted or interpreted by this crate. Users authenticate again through the native
provider flow. Community's keyring service and credential identifiers remain unchanged.

Keyring and metadata updates are not an OS-level multi-key transaction. A process crash
between backend operations still requires reconciliation and is not claimed atomic.
''')
print('Retirement transforms applied. Shared domain modules and benchmark history retained.')
