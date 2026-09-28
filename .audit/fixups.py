from pathlib import Path
import re, json, subprocess

def read(p): return Path(p).read_text()
def write(p,s): Path(p).write_text(s)
# Keep crate-level rustdoc before module items.
p='src-tauri/crates/profile-install/src/lib.rs'; s=read(p).replace('\nmod recovery_guard;\n','\n'); marker='use deltamod_installations_domain::'; s=s.replace(marker,'mod recovery_guard;\n\n'+marker,1); write(p,s)
# Remove the obsolete destructive reset button, not its neighboring controller/hash settings.
p='web/views/options/index.js'; s=read(p)
if "page('deleteall')" in s:
    hit=s.index("page('deleteall')"); start=s.rfind('            await addButton',0,hit)
    end=s.index('            await addCheckboxOption',hit)
    assert start>=0 and 'community_delete_data' in s[start:hit]
    s=s[:start]+s[end:]
write(p,s)
p='web/types/preload.d.ts'; s=read(p)
s=re.sub(r'^\s*openInstallationInUndertaleModTool[^\n]*\n','',s,flags=re.M)
s=re.sub(r'^\s*electronAPI[^\n]*\n','',s,flags=re.M)
write(p,s)
# Preserve the original one-line-per-test classification format.
p='scripts/tauri-parity/fixtures/test-classification.json'
s=subprocess.check_output(['git','show','HEAD:'+p],text=True)
s=re.sub(r'^.*"tests/credential-storage\.test\.js".*\n','',s,flags=re.M)
s=re.sub(r'^.*"retirementOccursInSeparateCleanupRelease".*\n','',s,flags=re.M)
s=re.sub(r',\n(\s*[}\]])',r'\n\1',s)
json.loads(s); write(p,s)
# A native OAuth policy assertion replaces the deleted Electron handler assertion.
p='tests/nexus-ui-policy.test.js'; s=read(p)
s=re.sub(r'^.*expect\(ipc\)\.toMatch\(/handle.*\n',"        expect(sources).not.toContain('modSources:setNexusKey');\n",s,flags=re.M)
s=re.sub(r'^.*expect\(ipc\)\.toMatch\(/getNexusAuthMethod.*\n',"        expect(ipc).toContain('CredentialKind::NexusOAuthTokens');\n        expect(ipc).toContain('CALLBACK_PORT: u16 = 52817');\n",s,flags=re.M)
write(p,s)
p='tests/g3mtool-provenance.test.js'; s=read(p)
start=s.index("    it('packages unsigned macOS"); end=s.index("    it('rejects an unapproved",start)
s=s[:start]+'''    it('packages Tauri macOS apps and DMGs with native patcher targets', () => {
        const mac = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri', 'tauri.macos.conf.json'), 'utf8'));
        expect(packageJson.scripts['build-macos']).toBe('npm run build:tauri');
        expect(mac.bundle.category).toBe('Utility');
        expect(mac.bundle.targets).toEqual(['app', 'dmg']);
        expect(TARGETS).toHaveProperty('darwin-x64');
        expect(TARGETS).toHaveProperty('darwin-arm64');
    });

'''+s[end:]; write(p,s)
# No bootstrap workflow may reconstruct or release the retired Electron application.
p=Path('.github/workflows/bootstrap-2.0.11.yml')
if p.exists():
    assert 'electron' in p.read_text().lower() or 'release.yml' in p.read_text()
    p.unlink()
# The browser fixture already defines the real backend; remove only its redundant alias.
p='tests/e2e/fixtures/refinement-page.js'; s=read(p)
s=re.sub(r'^\s*window\.deltamodBackend\s*=\s*\{\s*invoke\s*\};?\s*\n','',s,flags=re.M)
write(p,s)
# Update scalar count assertions alongside the structured reports.
for p in [Path('scripts/tauri-parity/test/harness.test.js'),Path('tests/tauri-parity-classification.test.js')]:
    s=p.read_text()
    s=s.replace('counts.rendererInvoke, 129','counts.rendererInvoke, 123').replace('counts.rustKnown, 129','counts.rustKnown, 123').replace('counts.rustUnsupported, 6','counts.rustUnsupported, 0')
    p.write_text(s)
print('Retired control consumers and packaging assertions reconciled without dropping shared tests.')
