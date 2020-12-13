use atomicwrites::{AllowOverwrite, AtomicFile};
use getopt::Opt;
use git2::{Config, Error as GitError, ErrorClass, ErrorCode, Repository};
use std::{
    collections::HashSet,
    env,
    error::Error,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

program::main!("git-ignore");

fn usage_line() -> String {
    format!(
        "Usage: {} [-h] [-gir] [-f FILE] pattern [pattern ...]",
        program::name("git-ignore")
    )
}

fn print_usage() {
    println!("{}", usage_line());
    println!("  -f FILE  add patterns to FILE");
    println!("  -g       add patterns to global ignore file (core.excludesFile)");
    println!("  -i       add patterns to internal repository ignore file (_/.git/info/exclude)");
    println!("  -r       add patterns to root-level repository ignore file (_/.gitignore)");
    println!();
    println!("  -h       display this help");
    println!();
    println!("By default, patterns are added to the file '.gitignore' in the current directory.");
    println!("The specified file is created if it does not exist.");
}

fn program() -> program::Result {
    let mut args = program::args();
    let mut opts = getopt::Parser::new(&args, "f:ghir");
    let mut mode = Mode::File(".gitignore".to_string());

    loop {
        match opts.next().transpose()? {
            None => break,
            Some(opt) => match opt {
                Opt('f', Some(arg)) => mode = Mode::File(arg),
                Opt('g', None) => mode = Mode::Global,
                Opt('i', None) => mode = Mode::Internal,
                Opt('r', None) => mode = Mode::Root,
                Opt('h', None) => {
                    print_usage();
                    return Ok(0);
                }
                _ => unreachable!(),
            },
        }
    }

    let args = args.split_off(opts.index());
    if args.is_empty() {
        eprintln!("{}", usage_line());
        return Ok(1);
    }

    update(mode, args)
}

enum Mode {
    File(String),
    Global,
    Internal,
    Root,
}

fn update(mode: Mode, args: Vec<String>) -> program::Result {
    let file = get_file(mode)?;

    let old = fs::read_to_string(&file).or_else(|e| {
        if e.kind() != io::ErrorKind::NotFound {
            return Err(e);
        }

        fs::File::create(&file)?;
        Ok("".to_string())
    })?;

    eprint!("Updating {}... ", file.to_string_lossy());
    let new = merge(&old, &args);

    if new == old {
        eprintln!("Nothing to do!");
    } else {
        AtomicFile::new(&file, AllowOverwrite).write(|f| f.write_all(new.as_bytes()))?;
        eprintln!("Done!");
    }

    Ok(0)
}

fn get_file(mode: Mode) -> Result<PathBuf, Box<dyn Error>> {
    match mode {
        Mode::File(name) => Ok(env::current_dir()?.join(name)),
        Mode::Global => global_ignore_file(),
        Mode::Internal => internal_ignore_file(),
        Mode::Root => root_ignore_file(),
    }
}

fn global_ignore_file() -> Result<PathBuf, Box<dyn Error>> {
    match Config::open_default()?.get_path("core.excludesFile") {
        Err(error) => {
            if error.class() == ErrorClass::Config && error.code() == ErrorCode::NotFound {
                let dir = dirs::config_dir()
                    .ok_or_else(|| Box::new(GitError::from_str("Could not find XDG_CONFIG_HOME")))?
                    .join("git");
                fs::create_dir_all(&dir)?;
                Ok(dir.join("ignore"))
            } else {
                Err(Box::new(error))
            }
        }
        Ok(path) => Ok(path),
    }
}

// `Repository::open_from_env()?.path()` returns a path that uses '/' on Windows; fix that
fn fix_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let mut new = PathBuf::new();
    for e in path.as_ref().iter() {
        new.push(e)
    }

    new
}

fn internal_ignore_file() -> Result<PathBuf, Box<dyn Error>> {
    let dir = fix_path(Repository::open_from_env()?.path()).join("info");
    fs::create_dir_all(&dir)?;
    Ok(dir.join("exclude"))
}

fn root_ignore_file() -> Result<PathBuf, Box<dyn Error>> {
    match Repository::open_from_env()?.workdir() {
        None => Err(Box::new(GitError::from_str("Repository is bare"))),
        Some(path) => Ok(path.join(".gitignore")),
    }
}

fn merge(text: &str, args: &[String]) -> String {
    let mut lines: HashSet<String> = text.lines().map(String::from).collect();

    for arg in args {
        lines.insert(arg.to_string());
    }

    let mut lines: Vec<String> = lines
        .into_iter()
        .filter_map(|line| {
            let line = line.trim().to_string();

            if line.is_empty() || line.starts_with('#') {
                None
            } else {
                Some(line)
            }
        })
        .collect();

    let lines = lines.as_mut_slice();
    lines.sort_unstable();

    let mut lines = lines.to_vec();
    lines.dedup();

    let (neg, pos): (Vec<String>, Vec<String>) =
        lines.iter().cloned().partition(|l| l.starts_with('!'));
    lines.clear();
    lines.extend(pos);
    lines.extend(neg);

    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }

    text
}
