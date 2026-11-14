mod action;
mod clap;
mod config;
mod crokey;
mod execute;
pub mod formatter;
mod list;
mod logger;
#[cfg(feature = "mlua")]
mod lua;
mod markdown;
mod pager;
mod parse;
mod paths;
mod register;
mod sort;
mod start;
mod utils;

use clap::*;
use config::PartialConfig;
use logger::*;
use paths::*;
use start::*;
use utils::*;

use std::{
    io::{self, Read, Write},
    path::Path,
    process::exit,
};
use strum::IntoEnumIterator;

use cba::{
    _dbg, bait::ResultExt, bog::BogOkExt, bring::split::split_on_delimiter_with_doubled_escape,
    ebog,
};

use matchmaker::MatchError;
use matchmaker_partial::Set;

use crate::parse::{get_pairs, try_split_kv};

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // Classic Unix behavior: die with SIGPIPE when a downstream reader
    // (`mm ... | head`) closes the pipe, instead of surfacing EPIPE errors
    // from subsequent writes. Rust ignores the signal by default.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL)
    };

    let (cli, config_args) = Cli::get_partitioned_args();

    init_logger(
        [cli.quiet, cli.verbose],
        &state_dir().join(format!("{BINARY_SHORT}.log")),
    );
    log::debug!("{cli:?}, {config_args:?}");

    display_doc(&cli);
    if let Some(download) = &cli.download {
        // Empty string here means "download all"; a non-empty value is a
        // folder name or a file preset (with or without `.toml`).
        handle_download(
            download,
            &["jpg", "png", "gif", "gitignore", "gitattributes"],
        );
    }

    if let Some(image) = &cli.image {
        // The positional args are handed to the image dispatcher instead of
        // being parsed as config overrides; `-q` keeps it from falling back to
        // `chafa`.
        handle_image(image, &config_args, cli.quiet > 0);
    }

    if cli.presets {
        let presets_dir = presets_path();
        match preset_paths() {
            Ok(paths) => {
                let stdout = io::stdout();
                let mut stdout = stdout.lock();
                for path in paths {
                    if writeln!(stdout, "{}", path.display()).is_err() {
                        break;
                    }
                }
                exit(0);
            }
            Err(error) => {
                eprintln!(
                    "failed to list presets in {}: {error}",
                    presets_dir.display()
                );
                exit(1);
            }
        }
    }

    // get config overrides
    let partial = if config_args.is_empty() {
        None
    } else {
        Some(get_partial(config_args).__ebog())
    };

    let no_read = cli.no_read;
    let list = cli.list.clone();
    let context = cli.context as usize;
    // get config
    let config = enter(cli, partial).__ebog();

    // --list: output the command's output without starting the matcher
    if let Some(l) = &list {
        if l.is_empty() {
            crate::list::list(config)
        } else {
            crate::list::list_arg(config, l)
        }
    }

    // begin
    match start(config, no_read, context).await {
        Ok(_) => {
            log::debug!("Execution Complete");
        }
        Err(err) => match err {
            MatchError::Abort(i) => {
                exit(i);
            }
            MatchError::Become(code) => {
                exit(code as i32);
            }
            MatchError::EventLoopClosed => {
                exit(125);
            }
            MatchError::TUIError(e) => {
                ebog!("TUI"; "{e}");
                // Hard failure; 2 keeps it disjoint from user aborts
                // (`Quit`) and the no-match codes.
                exit(2)
            }
            MatchError::NoMatch => {
                let e = std::mem::take(&mut *CHUNK_ERROR.lock().unwrap());
                if e.is_empty() {
                    ebog!("NoMatch");
                    exit(148);
                } else {
                    ebog!("ChunkError"; "{e}");
                    exit(144);
                }
            }
            _ => unreachable!(),
        },
    };
}

fn get_partial(config_args: Vec<String>) -> anyhow::Result<PartialConfig> {
    let split = get_pairs(config_args)?;
    log::trace!("{split:?}");
    let mut partial = PartialConfig::default();
    for (path, val) in split {
        if !path.is_empty() && (path[0] == "env" || path[0] == "envs") {
            cba::wbog!(
                "Ignoring manual override of environment variables via CLI: {}",
                path.join(".")
            );
            continue;
        }

        let parts = {
            let mut parts = split_on_delimiter_with_doubled_escape(&val, ',');
            let is_binds = path.len() == 1 && ["binds", "b"].contains(&path[0].as_ref());
            try_split_kv(&mut parts, is_binds)?;
            parts
        };

        _dbg!(&path; &parts);
        partial
            .set(path.as_slice(), &parts)
            .prefix(format!("Invalid value for {}", path.join(".")))?;
    }

    Ok(partial)
}

/// The documentation shipped with the binary. Anything else is a path.
#[derive(Clone, Copy, strum::Display, strum::EnumIter, strum::EnumString)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
enum Doc {
    Options,
    Binds,
    Template,
    #[strum(serialize = "execute", serialize = "lua")]
    Execute,
    Other,
    Pager,
    Image,
}

impl Doc {
    fn text(self) -> &'static str {
        match self {
            Doc::Options => include_str!("../assets/docs/options.md"),
            Doc::Binds => include_str!("../assets/docs/binds.md"),
            Doc::Template => include_str!("../assets/docs/template.md"),
            Doc::Execute => include_str!("../assets/docs/execute.md"),
            Doc::Other => include_str!("../assets/docs/other.md"),
            Doc::Pager => include_str!("../assets/docs/pager.md"),
            Doc::Image => include_str!("../assets/docs/image.md"),
        }
    }
}

/// Builtin documentation, or a markdown file (`-` for stdin).
fn load_doc(doc: &str) -> Result<String, String> {
    if let Ok(doc) = doc.parse::<Doc>() {
        return Ok(doc.text().to_string());
    }
    if doc.is_empty() {
        return Err(format!(
            "`--doc` needs a builtin doc ({}), a markdown path, or `-` for stdin",
            builtin_docs()
        ));
    }
    if doc == "-" {
        let mut md = String::new();
        io::stdin()
            .read_to_string(&mut md)
            .map_err(|e| format!("Failed to read stdin: {e}"))?;
        return Ok(md);
    }
    std::fs::read_to_string(doc).map_err(|e| format!("Unknown doc `{doc}`: {e}"))
}

/// The builtin doc names, in the order errors list them.
fn builtin_docs() -> String {
    Doc::iter()
        .map(|doc| doc.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn display_doc(cli: &Cli) {
    let Some(doc) = cli.doc.as_deref() else {
        return;
    };
    let md = match load_doc(doc) {
        Ok(md) => md,
        Err(e) => {
            ebog!("{e}");
            exit(2)
        }
    };

    // best effort: docs stay renderable even when the config does not parse
    let cfg_path = cli
        .config
        .as_deref()
        .map(Path::new)
        .unwrap_or_else(|| default_config_path());
    let width = atty::is(atty::Stream::Stdout).then(|| termimad::terminal_size().0 as usize);
    let text = markdown::MarkdownConfig::load(cfg_path).render(&md, width);
    let ansi = matchmaker::utils::text_to_ansi(&text);
    if cfg!(feature = "pager") && atty::is(atty::Stream::Stdout) {
        #[cfg(feature = "pager")]
        crate::pager::minus_text(&ansi, &crate::config::PagerConfig::default());
        exit(0);
    }
    println!("{ansi}");
    exit(0)
}
