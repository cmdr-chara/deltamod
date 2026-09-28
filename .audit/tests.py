from pathlib import Path
import re

def read(p): return Path(p).read_text()
def write(p,s):
    p=Path(p); p.parent.mkdir(parents=True,exist_ok=True); p.write_text(s)

write('src-tauri/crates/updater-launch-runtime/src/hardening_tests.rs',r'''use super::*;
use std::sync::{atomic::{AtomicUsize, Ordering}, mpsc};
use std::time::Duration;

fn status(success: bool) -> ExitStatus {
    #[cfg(unix)] { use std::os::unix::process::ExitStatusExt; ExitStatus::from_raw(if success { 0 } else { 256 }) }
    #[cfg(windows)] { use std::os::windows::process::ExitStatusExt; ExitStatus::from_raw(if success { 0 } else { 1 }) }
}
struct ImmediateChild;
impl ChildProcess for ImmediateChild {
    fn wait(&mut self) -> io::Result<ExitStatus> { Ok(status(true)) }
    fn kill(&mut self) -> io::Result<()> { Ok(()) }
}
#[derive(Default)]
struct Spawner(AtomicUsize);
impl ProcessSpawner for Spawner {
    fn spawn(&self, _: &LaunchSpec) -> Result<Box<dyn ChildProcess>, LaunchError> {
        self.0.fetch_add(1, Ordering::SeqCst); Ok(Box::new(ImmediateChild))
    }
}
#[derive(Default)]
struct Steam(Mutex<Vec<String>>);
impl SteamOpener for Steam {
    fn open(&self, uri: &SteamUri) -> Result<(), SteamError> { self.0.lock().unwrap().push(uri.as_str().into()); Ok(()) }
}
fn fixture(host: HostPlatform, steam: bool) -> (tempfile::TempDir, GameRuntimeConfig) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let games = root.join("games"); let game = root.join("game");
    fs::create_dir(&games).unwrap(); fs::create_dir(&game).unwrap();
    fs::create_dir_all(game.join("GAME.app/Contents/MacOS")).unwrap();
    fs::write(game.join("GAME.app/Contents/MacOS/GAME"), b"fixture").unwrap();
    fs::write(game.join("GAME.exe"), b"fixture").unwrap();
    fs::write(game.join("GAME"), b"fixture").unwrap();
    fs::write(game.join("data.win"), b"original game data").unwrap();
    fs::write(games.join("game.json"), serde_json::to_vec(&json!({
        "id":"test-game", "platforms":{
            "win32":{"executable":"GAME.exe","dataFiles":["data.win"]},
            "linux":{"executable":"GAME","dataFiles":["data.win"]},
            "darwin":{"executable":"GAME.app/Contents/MacOS/GAME","dataFiles":["data.win"],"bundle":"GAME.app"}
        }
    })).unwrap()).unwrap();
    let store = root.join("store.json");
    fs::write(&store, serde_json::to_vec(&json!({"gamePid":"test-game","gamePath":game,"gamePlatform":host.as_legacy(),"isSteam":steam,"steamAppId":"1671210"})).unwrap()).unwrap();
    (temp, GameRuntimeConfig::new(games, store, host))
}
#[test]
fn steam_dispatch_uses_the_opener_on_every_supported_host() {
    for host in [HostPlatform::Win32, HostPlatform::Linux, HostPlatform::Darwin] {
        let (_temp, config) = fixture(host, true);
        let spawner = Arc::new(Spawner::default()); let steam = Arc::new(Steam::default());
        let runtime = GameRuntime::with_adapters(config, spawner.clone(), steam.clone(), Arc::new(NoopGameLifecycle));
        assert_eq!(runtime.dispatch("startGame", &[]).unwrap(), Some(json!(true)));
        assert_eq!(spawner.0.load(Ordering::SeqCst), 0);
        assert_eq!(*steam.0.lock().unwrap(), vec!["steam://rungameid/1671210"]);
        assert!(runtime.uses_external_launcher().unwrap());
    }
}
#[test]
fn steam_uri_rejects_suffixes_and_noncanonical_numeric_input() {
    for raw in ["", "0", "+1", "-1", " 1", "4294967296", "1/anything", "1?args=bad", "1#bad", "1\n", "１２", "00000000001"] {
        assert!(SteamUri::parse(format!("steam://run/{raw}")).is_err(), "{raw:?}");
    }
    for id in [1, 1671210, u32::MAX] { assert!(SteamUri::parse(format!("steam://rungameid/{id}")).is_ok()); }
    let mut state = 1_u32;
    for _ in 0..10_000 {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        let id = state.max(1);
        assert!(SteamUri::parse(format!("steam://run/{id}")).is_ok());
        assert!(SteamUri::parse(format!("steam://run/{id}/extra")).is_err());
    }
}
#[test]
fn failed_unix_opener_is_not_reported_as_success() {
    assert!(require_steam_success(status(true)).is_ok());
    assert!(require_steam_success(status(false)).is_err());
}
struct Finalization {
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl GameLifecycle for Finalization {
    fn finished(&self, _: bool) {
        self.entered.send(()).unwrap();
        self.release.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
#[test]
fn a_new_game_cannot_start_while_patch_finalization_is_running() {
    let (_temp, config) = fixture(HostPlatform::Win32, false);
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::channel();
    let runtime = GameRuntime::with_adapters(config, Arc::new(Spawner::default()), Arc::new(Steam::default()),
        Arc::new(Finalization { entered: entered_tx, release: Mutex::new(release_rx) }));
    runtime.start_game().unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let blocked = matches!(runtime.start_game(), Err(GameError::AlreadyRunning));
    let running = runtime.is_running();
    release_tx.send(()).unwrap();
    assert!(blocked && running);
}
#[test]
fn linux_binaries_are_not_passed_to_the_shell() {
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("runner");
    let mut resolution = GameResolution { root: root.path().to_path_buf(), platform: "linux".into(), executable: binary.clone(), bundle: None };
    let spec = resolution.launch_spec(HostPlatform::Linux).unwrap();
    assert_eq!(spec.executable, binary); assert!(spec.args.is_empty());
    resolution.executable = root.path().join("start.sh");
    let script = resolution.launch_spec(HostPlatform::Linux).unwrap();
    assert_eq!(script.executable, PathBuf::from("sh"));
    assert_eq!(script.args, vec![resolution.executable.to_string_lossy().into_owned()]);
}
#[test]
fn catalogue_and_store_input_sizes_are_bounded() {
    let (_temp, config) = fixture(HostPlatform::Linux, false);
    for i in 0..MAX_CATALOG_FILES { fs::write(config.games_dir.join(format!("extra-{i}.json")), b"{}").unwrap(); }
    let runtime = GameRuntime::new(config.clone());
    assert!(matches!(runtime.catalog(), Err(GameError::CatalogLimit)));
    fs::write(&config.store_path, vec![b' '; MAX_JSON_BYTES as usize + 1]).unwrap();
    assert!(runtime.uses_external_launcher().is_err());
}
#[cfg(unix)]
#[test]
fn invalid_unicode_environment_is_handled_without_panicking() {
    use std::os::unix::ffi::OsStringExt;
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "hardening_tests::non_unicode_environment_child"])
        .env("DELTAMOD_ENV_TEST_CHILD", "1")
        .env("PATH", std::ffi::OsString::from_vec(vec![0xff]))
        .env(std::ffi::OsString::from_vec(vec![0xfe]), "ignored")
        .status().unwrap();
    assert!(result.success());
}
#[cfg(unix)]
#[test]
fn non_unicode_environment_child() {
    if std::env::var_os("DELTAMOD_ENV_TEST_CHILD").is_none() { return; }
    assert!(!inherited_non_secret_environment().contains_key("PATH"));
}
''')

p='tests/tauri-adapter.test.js'; s=read(p)
s=s.replace("it('does nothing when the Electron preload bridge is present'", "it('is idempotent when the Tauri bridge is already installed'")
s=s.replace("    'sampleError'\n", "    'sampleError',\n    'rebootDev', 'createInstallLink', 'undertaleModTool:openInstallation',\n    'gamebanana_downloadAllInCollection', 'npsCallback', 'initialize'\n")
s=s.replace("        path.join(webRoot, 'preload.js'),\n",'')
a="        expect(root.__TAURI__.core.invoke).toHaveBeenNthCalledWith(2, 'backend_invoke', {\n            channel: 'undertaleModTool:openInstallation', data: ['2']\n        });"
assert a in s
s=s.replace(a,"        expect(root.communityAPI.tools).not.toHaveProperty('openInstallationInUndertaleModTool');\n        expect(root).not.toHaveProperty('electronAPI');")
s=s.replace('        unsubscribe();\n        pending.resolve(unlisten);', '        unsubscribe();\n        tauriHandler({ payload: { stale: true } });\n        expect(callback).toHaveBeenCalledTimes(1);\n        pending.resolve(unlisten);')
write(p,s)
p='tests/tauri-renderer-gates.test.js'; s=read(p)
s=s.replace("'disables beta-facing unsupported installation, account, tool, and download controls'","'gates native capabilities and omits retired installation actions'")
s=s.replace("'removeSteamIntegration', 'rebootDev', 'installDeltamodCLI'", "'removeSteamIntegration', 'installDeltamodCLI'")
s=s.replace('expect(installations).toContain("isCommandAvailable(\'createInstallLink\')");','expect(installations).not.toContain("createInstallLink");')
s=s.replace('expect(installations).toContain("isCommandAvailable(\'undertaleModTool:openInstallation\')");','expect(installations).not.toContain("undertaleModTool:openInstallation");')
s=s.replace('expect(collections).toContain("isCommandAvailable(\'gamebanana_downloadAllInCollection\')");','expect(collections).not.toContain("gamebanana_downloadAllInCollection");')
s=s.replace("        const deleteAll = read('web/views/deleteall/index.js');", "        expect(fs.existsSync(path.join(root, 'web/views/deleteall'))).toBe(false);")
s=re.sub(r'^.*expect\(deleteAll\).*\n','',s,flags=re.M)
s=s.replace('expect(patching).toContain("invokeOptional(\'npsCallback\'");','expect(patching).not.toContain("npsCallback");')
s=s.replace('        expect(patching).toContain("isCommandAvailable(\'npsCallback\')");\n','')
write(p,s)
print('Native launch and shared renderer retirement regressions added.')
