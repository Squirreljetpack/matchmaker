//! Behavioural tests for the portable module set.

use std::path::{Path, PathBuf};

use matchmaker_mlua_bindings::{register_lunacolors, register_modules};
use mlua::{FromLuaMulti, Lua, Table, Value};

/// A Lua VM with the bundled modules registered, optionally interactive.
fn vm(is_interactive: bool) -> Lua {
    let lua = Lua::new();
    register_modules(&lua, is_interactive).unwrap();
    lua
}

fn eval<T: FromLuaMulti>(lua: &Lua, code: &str) -> T {
    lua.load(code)
        .eval()
        .unwrap_or_else(|e| panic!("lua error for {code}: {e}"))
}

/// Quote a Rust string as a Lua literal.
fn lua_str(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

fn lua_path(p: &Path) -> String {
    lua_str(&p.to_string_lossy())
}

static TEMP_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A unique temp directory removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let n = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p =
            std::env::temp_dir().join(format!("mm-mlua-bindings-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn lua_literal(&self) -> String {
        lua_path(&self.0)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn lunacolors_global_and_require() {
    let lua = Lua::new();
    register_lunacolors(&lua).unwrap();

    let version: String = eval(&lua, "return lunacolors.version");
    assert_eq!(version, "v0.2.1");

    let red: String = eval(&lua, "return lunacolors.red('x')");
    assert_eq!(red, "\x1b[31mx\x1b[39m");

    let formatted: String = eval(&lua, "return lunacolors.format('{bold}x')");
    assert_eq!(formatted, "\x1b[1mx\x1b[0m");

    let same: bool = eval(
        &lua,
        "return lunacolors == Lunacolors \
         and require('lunacolors') == lunacolors \
         and require('Lunacolors') == Lunacolors",
    );
    assert!(same);
}

#[test]
fn util_sleep_uuid_timestamp() {
    let lua = vm(false);

    let start = std::time::Instant::now();
    let _: Value = eval(&lua, "return util.sleep(5)");
    assert!(start.elapsed() >= std::time::Duration::from_millis(5));

    let uuid: String = eval(&lua, "return util.uuid()");
    assert_eq!(uuid.len(), 36);
    let groups: Vec<&str> = uuid.split('-').collect();
    assert_eq!(
        groups.iter().map(|g| g.len()).collect::<Vec<_>>(),
        [8, 4, 4, 4, 12]
    );
    assert!(uuid.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    assert_eq!(&uuid[14..15], "4", "uuid version nibble");
    assert!(
        ["8", "9", "a", "b"].contains(&&uuid[19..20]),
        "uuid variant nibble"
    );

    let unix: String = eval(&lua, "return util.timestamp('unix')");
    assert!(unix.parse::<u64>().unwrap() > 1_600_000_000);

    let epoch: String = eval(&lua, "return util.timestamp('epoch')");
    assert_eq!(unix.len(), epoch.len());

    let ms: String = eval(&lua, "return util.timestamp('ms')");
    assert!(ms.parse::<u64>().unwrap() > 1_600_000_000_000);

    let year: String = eval(&lua, "return util.timestamp('%Y')");
    assert_eq!(year.len(), 4);
    assert!(year.parse::<u32>().unwrap() >= 2024);

    let now: String = eval(&lua, "return util.timestamp()");
    assert!(!now.is_empty());
}

#[test]
fn util_serde_round_trips() {
    let lua = vm(false);

    let json: String = eval(&lua, "return util.json_encode({ a = 1, b = { 2, 3 } })");
    let decoded: Table = eval(
        &lua,
        &format!("return util.json_decode({})", lua_str(&json)),
    );
    assert_eq!(decoded.get::<i64>("a").unwrap(), 1);

    let nested: i64 = eval(
        &lua,
        &format!("return util.json_decode({}).b[2]", lua_str(&json)),
    );
    assert_eq!(nested, 3);

    let toml: String = eval(&lua, "return util.toml_encode({ a = 1, b = 'x' })");
    assert!(toml.contains("a = 1"));
    let decoded: Table = eval(
        &lua,
        &format!("return util.toml_decode({})", lua_str(&toml)),
    );
    assert_eq!(decoded.get::<i64>("a").unwrap(), 1);
    assert_eq!(decoded.get::<String>("b").unwrap(), "x");

    let plain: String = eval(&lua, "return util.shq('plain')");
    assert_eq!(plain, "plain");
    let spaced: String = eval(&lua, "return util.shq('a b')");
    assert_eq!(spaced, "'a b'");
    let quoted: String = eval(&lua, r#"return util.shq("it's")"#);
    assert_eq!(quoted, r#"'it'\''s'"#);

    let words: Table = eval(&lua, r#"return util.split_args("code --wait")"#);
    assert_eq!(words.get::<String>(1).unwrap(), "code");
    assert_eq!(words.get::<String>(2).unwrap(), "--wait");
    let grouped: Table = eval(&lua, r#"return util.split_args([[notepad "a b.txt"]])"#);
    assert_eq!(grouped.get::<String>(1).unwrap(), "notepad");
    assert_eq!(grouped.get::<String>(2).unwrap(), "a b.txt");
    let empty: Table = eval(&lua, "return util.split_args('')");
    assert_eq!(empty.raw_len(), 0);
    assert!(
        lua.load(r#"return util.split_args([["unbalanced]])"#)
            .eval::<Table>()
            .is_err()
    );
}

#[test]
fn path_components_and_queries() {
    let lua = vm(false);
    let dir = TempDir::new("path");
    let file = dir.join("hello.tar.gz");
    std::fs::write(&file, b"x").unwrap();
    let empty = dir.join("empty.txt");
    std::fs::write(&empty, b"").unwrap();
    let sub = dir.join("sub");
    std::fs::create_dir(&sub).unwrap();
    let sub_file = sub.join("nested.txt");
    std::fs::write(&sub_file, b"y").unwrap();

    let parent: String = eval(&lua, &format!("return path.parent({})", lua_path(&file)));
    assert_eq!(parent, dir.path().to_string_lossy());

    let base: String = eval(&lua, &format!("return path.base({})", lua_path(&file)));
    assert_eq!(base, "hello.tar.gz");

    let stem: String = eval(&lua, &format!("return path.stem({})", lua_path(&file)));
    assert_eq!(stem, "hello.tar");

    let ext: String = eval(&lua, &format!("return path.ext({})", lua_path(&file)));
    assert_eq!(ext, "gz");

    let no_ext: String = eval(&lua, "return path.ext('Makefile')");
    assert_eq!(no_ext, "");

    let joined: String = eval(
        &lua,
        &format!(
            "return path.join({}, 'sub', 'nested.txt')",
            dir.lua_literal()
        ),
    );
    assert_eq!(joined, sub_file.to_string_lossy());

    let is_file: bool = eval(&lua, &format!("return path.is_file({})", lua_path(&file)));
    assert!(is_file);
    let is_dir: bool = eval(&lua, &format!("return path.is_dir({})", lua_path(&sub)));
    assert!(is_dir);
    let not_file: bool = eval(&lua, &format!("return path.is_file({})", lua_path(&sub)));
    assert!(!not_file);

    let empty_file: bool = eval(&lua, &format!("return path.is_empty({})", lua_path(&empty)));
    assert!(empty_file);
    let nonempty_file: bool = eval(&lua, &format!("return path.is_empty({})", lua_path(&file)));
    assert!(!nonempty_file);
    let empty_dir: bool = eval(
        &lua,
        &format!("return path.is_empty({})", lua_path(&dir.join("sub"))),
    );
    assert!(!empty_dir, "sub holds nested.txt");
    let bare = dir.join("bare");
    std::fs::create_dir(&bare).unwrap();
    let bare_empty: bool = eval(&lua, &format!("return path.is_empty({})", lua_path(&bare)));
    assert!(bare_empty);

    let exists: bool = eval(&lua, &format!("return path.exists({})", lua_path(&file)));
    assert!(exists);
    let missing: bool = eval(
        &lua,
        &format!("return path.exists({})", lua_path(&dir.join("nope"))),
    );
    assert!(!missing);
    let lexists: bool = eval(&lua, &format!("return path.lexists({})", lua_path(&file)));
    assert!(lexists);

    let rel: String = eval(
        &lua,
        &format!(
            "return path.rel({}, {})",
            lua_path(&sub_file),
            dir.lua_literal()
        ),
    );
    assert_eq!(rel, Path::new("sub").join("nested.txt").to_string_lossy());

    let mime: Value = eval(&lua, "return path.mime");
    assert!(mime.is_nil());
}

#[test]
fn path_expand_tilde() {
    let lua = vm(false);

    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|h| !h.as_os_str().is_empty());
    let Some(home) = home else {
        return;
    };
    let home = home.to_string_lossy().into_owned();

    let bare: String = eval(&lua, "return path.expand_tilde('~')");
    assert_eq!(bare, home);

    let nested: String = eval(&lua, "return path.expand_tilde('~/foo')");
    assert_eq!(nested, format!("{home}/foo"));

    let other_user: String = eval(&lua, "return path.expand_tilde('~other/foo')");
    assert_eq!(other_user, "~other/foo");

    let absolute: String = eval(&lua, "return path.expand_tilde('/abs/path')");
    assert_eq!(absolute, "/abs/path");
}

#[test]
fn fs_read_write_stat() {
    let lua = vm(false);
    let dir = TempDir::new("fs-rw");
    let file = dir.join("notes.txt");

    let wrote: bool = eval(
        &lua,
        &format!("return fs.write({}, 'hello world')", lua_path(&file)),
    );
    assert!(wrote);

    let content: String = eval(&lua, &format!("return fs.read({})", lua_path(&file)));
    assert_eq!(content, "hello world");

    let stat: Table = eval(&lua, &format!("return fs.stat({})", lua_path(&file)));
    assert_eq!(stat.get::<u64>("size").unwrap(), 11);
    assert!(stat.get::<bool>("is_file").unwrap());
    assert!(!stat.get::<bool>("is_dir").unwrap());
    assert!(!stat.get::<bool>("is_symlink").unwrap());
    assert!(stat.get::<u64>("mtime").unwrap() > 1_600_000_000);

    let missing: Value = eval(
        &lua,
        &format!("return fs.stat({})", lua_path(&dir.join("nope"))),
    );
    assert!(missing.is_nil());

    let read_err: mlua::Result<String> = lua
        .load(format!("return fs.read({})", lua_path(&dir.join("nope"))))
        .eval();
    assert!(read_err.is_err());
}

#[test]
fn fs_mkdir_readdir_remove() {
    let lua = vm(false);
    let dir = TempDir::new("fs-dirs");
    let a = dir.join("a");
    let b = a.join("b");

    let made: bool = eval(&lua, &format!("return fs.mkdir({})", lua_path(&b)));
    assert!(made);
    assert!(b.is_dir());

    let non_recursive: bool = eval(
        &lua,
        &format!("return fs.mkdir({}, false)", lua_path(&dir.join("single"))),
    );
    assert!(non_recursive);
    let non_recursive_err: mlua::Result<bool> = lua
        .load(format!(
            "return fs.mkdir({}, false)",
            lua_path(&dir.join("x").join("y"))
        ))
        .eval();
    assert!(non_recursive_err.is_err());

    std::fs::write(b.join("one.txt"), b"1").unwrap();
    std::fs::write(b.join("two.txt"), b"2").unwrap();

    let mut entries: Vec<String> = eval(&lua, &format!("return fs.readdir({})", lua_path(&b)));
    entries.sort();
    assert_eq!(entries, ["one.txt", "two.txt"]);

    let removed: bool = eval(&lua, &format!("return fs.remove({}, true)", lua_path(&a)));
    assert!(removed);
    assert!(!a.exists());

    std::fs::write(dir.join("plain.txt"), b"x").unwrap();
    let removed_file: bool = eval(
        &lua,
        &format!("return fs.remove({})", lua_path(&dir.join("plain.txt"))),
    );
    assert!(removed_file);
    assert!(!dir.join("plain.txt").exists());
}

#[test]
fn fs_copy_rename_canonicalize() {
    let lua = vm(false);
    let dir = TempDir::new("fs-copy");
    let src = dir.join("src.txt");
    let dst = dir.join("dst.txt");
    let moved = dir.join("moved.txt");
    std::fs::write(&src, b"payload").unwrap();

    let copied: bool = eval(
        &lua,
        &format!("return fs.copy({}, {})", lua_path(&src), lua_path(&dst)),
    );
    assert!(copied);
    assert_eq!(std::fs::read_to_string(&dst).unwrap(), "payload");

    let renamed: bool = eval(
        &lua,
        &format!("return fs.rename({}, {})", lua_path(&dst), lua_path(&moved)),
    );
    assert!(renamed);
    assert!(!dst.exists());
    assert_eq!(std::fs::read_to_string(&moved).unwrap(), "payload");

    let canonical: String = eval(
        &lua,
        &format!("return fs.canonicalize({})", lua_path(&moved)),
    );
    assert_eq!(
        canonical,
        std::fs::canonicalize(&moved).unwrap().to_string_lossy()
    );

    let canonical_err: mlua::Result<String> = lua
        .load(format!(
            "return fs.canonicalize({})",
            lua_path(&dir.join("nope"))
        ))
        .eval();
    assert!(canonical_err.is_err());
}

#[cfg(unix)]
#[test]
fn fs_symlink_readlink() {
    let lua = vm(false);
    let dir = TempDir::new("fs-link");
    let target = dir.join("target.txt");
    let link = dir.join("link.txt");
    std::fs::write(&target, b"t").unwrap();

    let linked: bool = eval(
        &lua,
        &format!(
            "return fs.symlink({}, {})",
            lua_path(&target),
            lua_path(&link)
        ),
    );
    assert!(linked);

    let read: String = eval(&lua, &format!("return fs.readlink({})", lua_path(&link)));
    assert_eq!(read, target.to_string_lossy());

    let stat: Table = eval(&lua, &format!("return fs.stat({})", lua_path(&link)));
    assert!(stat.get::<bool>("is_symlink").unwrap());
    assert!(stat.get::<bool>("is_file").unwrap());
}

#[cfg(unix)]
#[test]
fn fs_remove_unlinks_symlinked_dir() {
    let lua = vm(false);
    let dir = TempDir::new("fs-remove-link");
    let target = dir.join("target");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("keep.txt"), b"keep").unwrap();
    let link = dir.join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let removed: bool = eval(&lua, &format!("return fs.remove({})", lua_path(&link)));
    assert!(removed);
    assert!(link.symlink_metadata().is_err(), "symlink should be gone");
    assert!(target.is_dir(), "target directory must survive");
    assert_eq!(
        std::fs::read_to_string(target.join("keep.txt")).unwrap(),
        "keep"
    );
}

#[cfg(unix)]
#[test]
fn fs_chmod() {
    use std::os::unix::fs::PermissionsExt;

    let lua = vm(false);
    let dir = TempDir::new("fs-chmod");
    let file = dir.join("run.sh");
    std::fs::write(&file, b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

    let stat: Table = eval(&lua, &format!("return fs.stat({})", lua_path(&file)));
    assert!(!stat.get::<bool>("executable").unwrap());

    let ok: bool = eval(&lua, &format!("return fs.chmod({}, '+x')", lua_path(&file)));
    assert!(ok);
    let stat: Table = eval(&lua, &format!("return fs.stat({})", lua_path(&file)));
    assert!(stat.get::<bool>("executable").unwrap());
    assert_eq!(stat.get::<u32>("mode").unwrap() & 0o111, 0o111);

    let ok: bool = eval(&lua, &format!("return fs.chmod({}, '-x')", lua_path(&file)));
    assert!(ok);
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o111,
        0
    );

    let ok: bool = eval(
        &lua,
        &format!("return fs.chmod({}, '0o600')", lua_path(&file)),
    );
    assert!(ok);
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let ok: bool = eval(&lua, &format!("return fs.chmod({}, 420)", lua_path(&file)));
    assert!(ok);
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o644
    );

    let bad: mlua::Result<bool> = lua
        .load(format!("return fs.chmod({}, true)", lua_path(&file)))
        .eval();
    assert!(bad.is_err());
}

#[test]
fn fs_temp_paths() {
    let lua = vm(false);

    let dir: String = eval(&lua, "return fs.tempdir()");
    let p = PathBuf::from(&dir);
    assert!(p.is_dir());
    assert!(
        p.file_name().unwrap().to_string_lossy().starts_with("tmp_"),
        "{dir}"
    );
    std::fs::remove_dir(&p).unwrap();

    let named: String = eval(&lua, "return fs.tempdir('mmtag_')");
    let p = PathBuf::from(&named);
    assert!(p.is_dir());
    assert!(
        p.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("mmtag_")
    );
    std::fs::remove_dir(&p).unwrap();

    let file: String = eval(&lua, "return fs.tempfile()");
    let p = PathBuf::from(&file);
    assert!(p.is_file());
    assert!(p.file_name().unwrap().to_string_lossy().starts_with("tmp_"));
    std::fs::remove_file(&p).unwrap();

    let named: String = eval(&lua, "return fs.tempfile('mmfile_')");
    let p = PathBuf::from(&named);
    assert!(p.is_file());
    assert!(
        p.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("mmfile_")
    );
    std::fs::remove_file(&p).unwrap();
}

#[test]
fn fs_temp_paths_reject_escaping_prefixes() {
    let lua = vm(false);

    for script in [
        "return fs.tempfile('../escape')",
        "return fs.tempdir('../escape')",
        "return fs.tempfile('/abs')",
        "return fs.tempdir('/abs')",
    ] {
        let res: mlua::Result<String> = lua.load(script).eval();
        let err = res.expect_err(script);
        assert!(
            err.to_string().contains("invalid temp prefix"),
            "{script}: {err}"
        );
    }

    #[cfg(windows)]
    {
        let res: mlua::Result<String> = lua.load("return fs.tempfile('C:evil')").eval();
        assert!(res.is_err(), "a drive-relative prefix must be rejected");
    }
}

#[cfg(unix)]
#[test]
fn proc_run_and_stdin() {
    let lua = vm(false);

    let out: Table = eval(&lua, "return proc.run('sh', { '-c', 'echo hi' })");
    assert!(out.get::<bool>("ok").unwrap());
    assert_eq!(out.get::<i64>("code").unwrap(), 0);
    assert_eq!(out.get::<String>("stdout").unwrap(), "hi\n");
    assert_eq!(out.get::<String>("stderr").unwrap(), "");

    let out: Table = eval(&lua, "return proc.run('sh', { '-c', 'exit 3' })");
    assert!(!out.get::<bool>("ok").unwrap());
    assert_eq!(out.get::<i64>("code").unwrap(), 3);

    let out: Table = eval(
        &lua,
        "return proc.run('cat', {}, { stdin = 'piped payload' })",
    );
    assert_eq!(out.get::<String>("stdout").unwrap(), "piped payload");

    let out: Table = eval(
        &lua,
        "return proc.run('sh', { '-c', 'echo $MM_MLUA_TEST' }, \
        { env = { MM_MLUA_TEST = 'from-env' } })",
    );
    assert_eq!(out.get::<String>("stdout").unwrap(), "from-env\n");

    let dir = TempDir::new("proc-cwd");
    let out: Table = eval(
        &lua,
        &format!(
            "return proc.run('sh', {{ '-c', 'pwd' }}, {{ cwd = {} }})",
            dir.lua_literal()
        ),
    );
    let printed = out.get::<String>("stdout").unwrap();
    assert_eq!(
        std::fs::canonicalize(printed.trim()).unwrap(),
        std::fs::canonicalize(dir.path()).unwrap()
    );

    let spawn: Table = eval(&lua, "return proc.spawn('sh', { '-c', 'exit 0' })");
    assert!(spawn.get::<bool>("ok").unwrap());
    assert!(spawn.get::<u32>("pid").unwrap() > 0);

    let bad: mlua::Result<Table> = lua
        .load("return proc.run('definitely-not-a-real-binary-mmlua')")
        .eval();
    assert!(bad.is_err());
}

#[test]
fn proc_has() {
    let lua = vm(false);

    let missing: bool = eval(
        &lua,
        "return proc.has('definitely-not-a-real-binary-mmlua')",
    );
    assert!(!missing);

    #[cfg(unix)]
    {
        let sh: bool = eval(&lua, "return proc.has('sh')");
        assert!(sh);
    }
}

#[test]
fn proc_exec_hands_the_process_over() {
    // `proc.exec` never returns, so it can only be observed from a child process:
    // re-run this test binary filtered to the child case and inspect that run.
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe)
        .args(["--exact", "proc_exec_child", "--nocapture"])
        .env("MM_MLUA_EXEC_CHILD", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("exec-ran"),
        "no child output; stdout: {stdout}"
    );
    assert_eq!(out.status.code(), Some(7), "stdout: {stdout}");
    assert!(
        !stdout.contains("test result"),
        "the harness came back, so exec did not replace the process; stdout: {stdout}"
    );
}

#[test]
fn proc_exec_child() {
    if std::env::var("MM_MLUA_EXEC_CHILD").is_err() {
        return;
    }
    #[cfg(unix)]
    let (prog, args) = ("sh", ["-c", "echo exec-ran; exit 7"]);
    #[cfg(windows)]
    let (prog, args) = ("cmd", ["/C", "echo exec-ran & exit 7"]);

    let args = args.map(lua_str).join(", ");
    let call = format!("proc.exec({}, {{ {args} }})", lua_str(prog));
    let lua = vm(false);
    eval::<()>(&lua, &call);
    panic!("proc.exec returned");
}

#[cfg(target_os = "linux")]
#[test]
fn proc_spawn_reaps_the_child() {
    let lua = vm(false);
    let spawn: Table = eval(&lua, "return proc.spawn('sh', { '-c', 'exit 0' })");
    let pid = spawn.get::<u32>("pid").unwrap();
    let proc_entry = format!("/proc/{pid}");
    for _ in 0..50 {
        if !Path::new(&proc_entry).exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("child {pid} was never reaped ({proc_entry} still exists)");
}

#[test]
fn modules_are_globals_and_preloaded() {
    let lua = vm(false);

    let same: bool = eval(
        &lua,
        "return require('fs') == fs and require('path') == path \
         and require('util') == util and require('proc') == proc",
    );
    assert!(same);

    let editable: i64 = eval(
        &lua,
        "fs.host_added = function() return 7 end\n\
         util.host_added = function() return 8 end\n\
         lunacolors.host_added = function() return 9 end\n\
         return require('fs').host_added() == 7 \
            and require('util').host_added() == 8 \
            and require('lunacolors').host_added() == 9 and 1 or 0",
    );
    assert_eq!(editable, 1);

    // Host-only functions are appended by the host, never by this crate.
    let excluded: bool = eval(
        &lua,
        "return path.mime == nil and fs.trash == nil \
         and util.clipboard_get == nil and util.clipboard_set == nil \
         and proc.editor == nil",
    );
    assert!(excluded);
}

#[test]
fn input_is_gated_on_interactive() {
    let headless = vm(false);
    let input: Value = eval(&headless, "return input");
    assert!(input.is_nil());
    let preload: Value = eval(&headless, "return package.preload.input");
    assert!(preload.is_nil());

    let interactive = vm(true);
    let present: bool = eval(
        &interactive,
        "return type(input) == 'table' \
         and type(input.prompt) == 'function' \
         and type(input.confirm) == 'function' \
         and type(input.select) == 'function'",
    );
    assert!(present);

    let same: bool = eval(&interactive, "return require('input') == input");
    assert!(same);

    let appended: String = eval(
        &interactive,
        "input.host_added = function() return 'host' end\n\
         return require('input').host_added()",
    );
    assert_eq!(appended, "host");
}
