use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::io::Read;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use std::{env, time};
use std::{io, path};
// use bincode::Encode;


/**
 * Design
 * - a daemon, listening on socket
 * - a cli, send to daemon via socket
 * - cli add a new process, daemon start it and add it to list
 * - daemon keep the list sync with config file
 * - if daemon start/stop, run/kill all processes on the list
 * -
 */

const SOCKET_PATH: &str = "/tmp/pm.sock";

use serde::{Deserialize, Serialize};

#[derive(Debug)]
struct ProcessChild {
    name: String,
    child: Child,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppConfig {
    /**
     * name act as a unique identifier of the app,
     * if create a app with same name, old one will be overwriten
     * app specific operation base on name,
     * e.g. enable name1, disable name2
     * If not provided, name will be same as path
     */
    pub name: String,
    pub cmd: String,
    pub args: Vec<String>,
    pub cwd: path::PathBuf,
    pub enabled: bool,
    pub logdir: Option<path::PathBuf>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /* this allows .toml to be empty */
    #[serde(default)]
    pub apps: Vec<AppConfig>,
    #[serde(skip)]
    config_filepath: path::PathBuf,
}


impl Config {
    pub fn load(&mut self) -> std::io::Result<()> {
        match fs::read_to_string(&self.config_filepath) {
            Ok(content) => {
                let config: Self = toml::from_str(&content)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                // Ok(config)
                self.apps = config.apps;
                Ok(())
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // File missing: use default
                // Ok(self.default())
                self.apps = self.default().apps;
                Ok(())
            }
            Err(err) => Err(err), // Propagate other errors
        }

        // let content = fs::read_to_string(&self.config_filepath)?;
        // let config = toml::from_str(&content)
        //     .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        // Ok(config)
    }

    pub fn save(&self) -> std::io::Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.config_filepath)
            .unwrap()
            .write_all(&content.into_bytes())
        // fs::write(&self.config_filepath, content)
    }
    pub fn add_config(&mut self, new_config: AppConfig) {
        if let Some(old_config) = self.find_config(&new_config.name) {
            *old_config = new_config;
        } else {
            self.apps.push(new_config);
        }
        self.save()
            .unwrap_or_else(|_| eprintln!("[pm][Error] save config failed"));
        // self.save().expect("???");
        // eprintln!("[pm][Error] the App already exists")
    }
    pub fn enable(&mut self, name: &str, enabled: bool) {
        if let Some(appconfig) = self.find_config(name) {
            appconfig.enabled = enabled;
        }
        self.save()
            .unwrap_or_else(|_| eprintln!("[pm][Error] save config failed"));
    }


    pub fn find_config(&mut self, name: &str) -> std::option::Option<&mut AppConfig> {
        self.apps.iter_mut().find(|i| i.name == name)
    }


    fn default(&self) -> Self {
        Self {
            apps: vec![],
            config_filepath: self.config_filepath.clone(),
        }
    }
}



fn main() -> std::io::Result<()> {
    register_sigint()?;

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <command>", &args[0]);
        return Ok(());
    }
    let command = &args[1];
    let params = &args[2..];

    if command == "daemon" {
        start_daemon()?;
        start_listening(
            params
                .get(0)
                .map(|s| Path::new(s))
                .unwrap_or(env::home_dir().unwrap().join("pm.toml").as_path()),
        )
    } else {
        start_cli(command, params)
    }
}


fn start_cli(command: &str, params: &[String]) -> std::io::Result<()> {
    match UnixStream::connect(SOCKET_PATH) {
        Ok(mut stream) => {
            bincode::encode_into_std_write(
                &std::iter::once(command.to_string())
                    .chain(params.iter().cloned())
                    .collect::<Vec<String>>(),
                &mut stream,
                bincode::config::standard(),
            )
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;


            /* read altogether, wait everything written */
            // let mut response = String::new();
            // stream.read_to_string(&mut response)?;
            // println!("{}", response.trim());

            /* read line by line, so each writeln will be printed immediately */
            let mut reader = BufReader::new(stream);
            loop {
                let mut response = String::new();
                let bytes_read = reader.read_line(&mut response)?;
                if bytes_read == 0 {
                    break; // EOF
                }
                println!("{}", response.trim_end());
            }
        }
        Err(e) => {
            let args: Vec<String> = env::args().collect();
            eprintln!(
                "[pm][Error] Failed to connect to daemon, make sure daemon is running. <{e}> \n\
                [pm][Info] Daemon can start with `{} daemon`",
                &args[0]
            );
        }
    };
    Ok(())
}

/**
 * start by `start_daemon()`
 */
fn start_listening(config_filepath: &path::Path) -> std::io::Result<()> {
    let config = Arc::new(Mutex::new(Config {
        apps: vec![],
        config_filepath: config_filepath.to_path_buf(),
    }));

    config
        .lock()
        .unwrap()
        .load()
        .expect("why load config failed?");

    let processes_table = Arc::new(Mutex::new(Vec::<ProcessChild>::new()));

    // for appconfig in &config.lock().unwrap().apps {
    //     if let Ok(child) = spawn_process(
    //         &appconfig.cmd,
    //         &appconfig.args,
    //         appconfig
    //             .logdir
    //             .clone()
    //             .unwrap_or("/tmp/".into())
    //             .join(appconfig.name.clone() + ".log"),
    //     ) {
    //         processes_table.lock().unwrap().push(ProcessChild {
    //             name: appconfig.name.clone(),
    //             child: child,
    //         });
    //     }
    // }

    start_all(config.clone(), processes_table.clone());
    start_watchdog_loop(config.clone(), processes_table.clone());


    // Remove previous socket file if it exists
    let _ = fs::remove_file(SOCKET_PATH);

    let listener = UnixListener::bind(SOCKET_PATH)?;
    println!("Daemon listening on {}", SOCKET_PATH);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let cloned_config = config.clone();
                let cloned_processes_table = processes_table.clone();
                thread::spawn(move || handle_client(stream, cloned_config, cloned_processes_table));
            }
            Err(e) => {
                eprintln!("Error: {e}");
            }
        }
    }
    Ok(())
}
fn start_daemon() -> std::io::Result<()> {
    println!("Try to daemonize...");
    /* TODO: better daemon log dir */
    let stdout = fs::File::create(env::home_dir().unwrap().join("pm-daemon.out")).unwrap();
    let stderr = fs::File::create(env::home_dir().unwrap().join("pm-daemon.err")).unwrap();

    let daemonize = daemonize::Daemonize::new()
        .pid_file("/tmp/pm-daemon.pid") // Every method except `new` and `start`
        // .chown_pid_file(true) // is optional, see `Daemonize` documentation
        .working_directory(".") // for default behaviour.
        // .user("nobody")
        // .group("daemon") // Group name
        // .group(2) // or group id.
        // .umask(0o777) // Set umask, `0o027` by default.
        .umask(0o000) // Set umask, `0o027` by default.
        .stdout(stdout) // Redirect stdout to `/tmp/daemon.out`.
        .stderr(stderr) // Redirect stderr to `/tmp/daemon.err`.
        .privileged_action(|| "Executed before drop privileges");

    match daemonize.start() {
        Ok(_) => {
            println!("Success, daemonized");
            Ok(())
        }
        Err(error) => {
            /* ugly hack */
            if error.to_string().contains("unable to lock pid file") {
                eprintln!("[pm][Warn] Daemon may be already started, try to restart");
                start_cli("quit", &[])?;
                /* TODO: wait until quit finished */

                /* TODO: will this cause infinite loop? */
                start_daemon()
                // daemonize
                //     .start()
                //     .map(|_| ())
                //     .map_err(|_| std::io::Error::new(io::ErrorKind::Other, "Daemon restart failed"))
            } else {
                eprintln!("Error!, {}", error);
                Err(io::Error::new(io::ErrorKind::Other, error))
            }
        }
    }

    // let daemon = daemonize_me::Daemon::new()
    //     .pid_file("/tmp/example.pid", Some(false))
    //     // .user(User::try_from("daemon").unwrap())
    //     // .group(Group::try_from("daemon").unwrap())
    //     .umask(0o000)
    //     .work_dir("/tmp")
    //     .stdout(stdout)
    //     .stderr(stderr)
    //     .start();
    // match daemon {
    //     Ok(_) => println!("Daemonized with success!"),
    //     Err(e) => {
    //         match e {
    //             daemonize_me::DaemonError::InitGroups => {},
    //             _ => {},
    //         }
    //         eprintln!("Error, {}", e)
    //     },
    // }
}

fn handle_client(
    mut stream: UnixStream,
    config: Arc<Mutex<Config>>,
    processes_table: Arc<Mutex<Vec<ProcessChild>>>,
) {
    let message: Vec<String> =
        bincode::decode_from_std_read(&mut stream, bincode::config::standard())
            .expect("decode_from_std_read failed, maybe interface changed?");

    let (command, params) = match message.as_slice() {
        [first, rest @ ..] => (first, rest),
        [] => panic!("Empty message received"),
    };

    match command.as_str() {
        "status" => {
            let _ = writeln!(stream, "Daemon is running.");
        }
        "add" => {
            if params.len() >= 2 {
                let app_name = &params[0];
                let _ = writeln!(
                    stream,
                    "Add new App {:?}, cmd: {:?}",
                    app_name,
                    &params[1..],
                );
                {
                    let mut config = config.lock().unwrap();
                    config.add_config(AppConfig {
                        name: params[0].to_string(),
                        cmd: params[1].to_string(),
                        args: params[2..].to_vec(),
                        cwd: PathBuf::from("/"), /* TODO: implement cwd */
                        enabled: true,
                        logdir: None,
                    });
                }
                /* TODO: restart instead of ignore if already started */
                let result = try_start_process_by_name(app_name, config, processes_table)
                    .unwrap_or_else(|e| e);
                let _ = writeln!(stream, "{result}");
            } else {
                let _ = writeln!(
                    stream,
                    "usage: add <name> </path/to/app> <param1> <param2> ..."
                );
            }
        }
        "remove" => {
            // TODO:
        }
        cmd if cmd.starts_with("l") /* "ls" */ => {
            /* TODO: show as a beautiful table */
            let _ = writeln!(stream, "{:#?}", processes_table.lock().unwrap());
            let _ = writeln!(stream, "{:#?}", config.lock().unwrap());
        }
        cmd if cmd.starts_with("r") /* "restart" */ => {
            if params.len() >= 1 {
                let app_name = &params[0];
                let _ = writeln!(stream, "Restarting {app_name}... ");

                let result =
                    try_stop_process(app_name, processes_table.clone()).unwrap_or_else(|e| e);
                let _ = writeln!(stream, "{result}");

                let result = try_start_process_by_name(app_name, config, processes_table)
                    .unwrap_or_else(|e| e);
                let _ = writeln!(stream, "{result}");
            } else {
                let _ = writeln!(stream, "usage: restart <name>");
            }
        }
        cmd if cmd.starts_with("e") /* "enable" */ => {
            if params.len() >= 1 {
                let app_name = &params[0];
                let _ = writeln!(stream, "Enable {app_name}");
                config.lock().unwrap().enable(&params[0], true);
                let result = try_start_process_by_name(app_name, config, processes_table)
                    .unwrap_or_else(|e| e);
                let _ = writeln!(stream, "{result}");
            } else {
                let _ = writeln!(stream, "usage: disable <name>");
            }
        }
        cmd if cmd.starts_with("d") /* "disable" */ => {
            if params.len() >= 1 {
                let app_name = &params[0];
                let _ = writeln!(stream, "Disable {app_name}");
                // thread::sleep(Duration::from_millis(2000));
                config.lock().unwrap().enable(&params[0], false);

                let result =
                    try_stop_process(app_name, processes_table.clone()).unwrap_or_else(|e| e);
                let _ = writeln!(stream, "{result}");

                /* this syntax also works, but... weird? */
                // let _ = try_stop_process(app_name, processes_table).map_err(|e| {
                //     let _ = writeln!(stream, "{e}");
                // });
            } else {
                let _ = writeln!(stream, "usage: disable <name>");
            }
        }
        /* spawn all */
        "on" => {}
        "quit" => {
            let _ = writeln!(stream, "Stop all...");
            stop_all(processes_table);
            let _ = writeln!(stream, "Bye~");
            std::process::exit(0);
        }
        cmd => {
            let _ = writeln!(stream, "Unknown command: {cmd}");
        }
    }
}

fn start_all(config: Arc<Mutex<Config>>, processes_table: Arc<Mutex<Vec<ProcessChild>>>) {
    let config = config.lock().unwrap();
    for app_config in &config.apps {
        let res = try_start_process(&app_config, processes_table.clone());
        println!("{}", res.unwrap_or_else(|e| e));
        match res {
            Ok(o) => {
                println!("{o}");
            }
            Err(e) => println!("[pm][Error] {e}"),
        }
    }
}
fn stop_all(processes_table: Arc<Mutex<Vec<ProcessChild>>>) {
    let app_names: Vec<String> = {
        let processes_table = processes_table.lock().unwrap();
        let app_names = processes_table.iter().map(|p| p.name.clone()).collect();
        app_names
    };

    for app_name in app_names {
        let _ = try_stop_process(&app_name, processes_table.clone());
    }
}

fn try_stop_process(
    app_name: &str,
    processes_table: Arc<Mutex<Vec<ProcessChild>>>,
) -> Result<&'static str, &'static str> {
    let index_in_table = processes_table
        .lock()
        .unwrap()
        .iter()
        .position(|process_child| process_child.name == app_name);

    if let Some(index) = index_in_table {
        {
            /* ISSUE: what's this? why this keep locked? */
            let mut table_lock = processes_table.lock().unwrap();
            /* kill must borrow as mutable */
            let _ = stop_process(
                &mut table_lock[index].child,
                time::Duration::from_millis(2000),
            );
        }
        processes_table.lock().unwrap().remove(index);
        Ok("Kill successfully")
    } else {
        Err("Seems not started, do nothing")
    }
}

fn try_start_process_by_name(
    app_name: &str,
    config: Arc<Mutex<Config>>,
    processes_table: Arc<Mutex<Vec<ProcessChild>>>,
) -> Result<&'static str, &'static str> {
    let mut config_lock = config.lock().unwrap();
    let app_config = config_lock.find_config(&app_name);
    if let Some(app_config) = app_config {
        try_start_process(app_config, processes_table.clone())
        // let _ = try_start_process(app_config, processes_table.clone());
    } else {
        Err("The App name can't be found in config")
    }
}

/**
 * this do some checks for you:
 * 1. the app is enabled
 * 2. the app is not started (i.e. not in the table)
 */
fn try_start_process(
    // app_name: &str,
    // config: Arc<Mutex<Config>>,
    app_config: &AppConfig,
    processes_table: Arc<Mutex<Vec<ProcessChild>>>,
) -> Result<&'static str, &'static str> {
    let app_name = &app_config.name;
    // let mut config_lock = config.lock().unwrap();
    // let app_config = config_lock.find_config(&app_name);


    // if let Some(app_config) = app_config {
    if app_config.enabled {
        let index_in_table = processes_table
            .lock()
            .unwrap()
            .iter()
            .position(|process_child| &process_child.name == app_name);

        if let None = index_in_table {
            // let _ = writeln!(stream, "Let's spawn");
            if let Ok(child) = spawn_process(
                &app_config.cmd,
                &app_config.args,
                app_config
                    .logdir
                    .clone()
                    .unwrap_or("/tmp/".into())
                    .join(app_config.name.clone() + ".log"),
            ) {
                processes_table.lock().unwrap().push(ProcessChild {
                    name: app_name.to_string(),
                    child: child,
                });
                Ok("Spawn successfully")
            } else {
                Err("Spawn failed")
            }
        } else {
            Err("The process has already been started")
        }
    } else {
        Err("The App is disabled")
    }
    // } else {
    //     Err("The App name can't be found in config")
    // }
}
fn start_watchdog_loop(config: Arc<Mutex<Config>>, processes_table: Arc<Mutex<Vec<ProcessChild>>>) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(3));
            let mut processes_table_lock = processes_table.lock().unwrap();
            for process_child in processes_table_lock.iter_mut() {
                if let Ok(Some(_)) = process_child.child.try_wait() {
                    /* exited */
                    eprintln!(
                        "[pm][Info] {} exited! try to restart...",
                        process_child.name
                    );
                    if let Some(app) = config.lock().unwrap().find_config(&process_child.name) {
                        if let Ok(child) = spawn_process(&app.cmd, &app.args, "/tmp/aaa.log") {
                            process_child.child = child;
                        }
                    }
                }
            }
        }
    });
}

fn spawn_process<S: AsRef<OsStr>, P: AsRef<Path>, I: IntoIterator<Item = S>>(
    program: S,
    args: I,
    log_file: P,
) -> std::io::Result<std::process::Child> {
    let log = File::create(log_file)?;
    /* TODO: implement cwd */
    Command::new(program)
        .args(args)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .spawn()
}

fn stop_process(
    process: &mut std::process::Child,
    nice_wait: time::Duration,
) -> Result<(), std::io::Error> {
    fn kill_process(process: &mut std::process::Child) -> Result<(), std::io::Error> {
        if let Ok(()) = process.kill() {
            process.wait()?;
        } else {
            println!("Process {} has already exited", process.id());
        }
        Ok(())
    }

    let pid = nix::unistd::Pid::from_raw(process.id() as i32);
    match nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT) {
        // match nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL) {
        Ok(()) => {
            let expire = time::Instant::now() + nice_wait;
            while let Ok(None) = process.try_wait() {
                if time::Instant::now() > expire {
                    break;
                }
                std::thread::sleep(nice_wait / 10);
            }
            if let Ok(None) = process.try_wait() {
                kill_process(process)?;
            }
        }
        Err(nix::Error::EINVAL) => {
            println!("Invalid signal. Killing process {}", pid);
            kill_process(process)?;
        }
        Err(nix::Error::EPERM) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Insufficient permissions to signal process {}", pid),
            ));
        }
        Err(nix::Error::ESRCH) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Process {} does not exist", pid),
            ));
        }
        Err(e) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Unexpected error {}", e),
            ));
        }
    };
    Ok(())
}

fn register_sigint() -> std::io::Result<()> {
    // ctrlc::set_handler(move || {
    //     println!("received Ctrl+C!");
    // })
    // .expect("Error setting Ctrl-C handler");


    let mut signals = signal_hook::iterator::Signals::new([signal_hook::consts::SIGINT])?;
    thread::spawn(move || {
        for sig in signals.forever() {
            println!("Received signal {:#?}", sig);
            if sig == signal_hook::consts::SIGINT {
                println!("Bye~");
                signal_hook::low_level::exit(0);
            }
        }
    });
    Ok(())
}
