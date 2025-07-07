use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::Read;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
// use bincode::Encode;


const SOCKET_PATH: &str = "/tmp/pm.sock";

use serde::{Deserialize, Serialize};

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
    pub path: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub enabled: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)] /* this allows .toml to be empty */ pub apps: Vec<AppConfig>,
    #[serde(skip)]
    config_filepath: String,
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
        let _ = start_daemon()?;
        start_listening()
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

            // writeln!(stream, "{command}")?;

            let mut response = String::new();
            stream.read_to_string(&mut response)?;

            // let mut reader = BufReader::new(stream);
            // reader.read_line(&mut response)?;

            println!("{}", response.trim());
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
fn start_listening() -> std::io::Result<()> {
    let config = Arc::new(Mutex::new(Config {
        apps: vec![],
        config_filepath: "./pm.toml".to_string(),
    }));
    config
        .lock()
        .unwrap()
        .load()
        .expect("why load config failed?");


    // Remove previous socket file if it exists
    let _ = fs::remove_file(SOCKET_PATH);

    let listener = UnixListener::bind(SOCKET_PATH)?;
    println!("Daemon listening on {}", SOCKET_PATH);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let cloned_config = config.clone();
                thread::spawn(move || handle_client(stream, cloned_config));
            }
            Err(e) => {
                eprintln!("Error: {e}");
            }
        }
    }
    Ok(())
}
fn start_daemon() -> std::io::Result<()> {
    let stdout = fs::File::create("/tmp/daemon.out").unwrap();
    let stderr = fs::File::create("/tmp/daemon.err").unwrap();

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
                eprintln!("[pm][Warn] Daemon may be already started, try restart");
                start_cli("quit", &[])?;
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

fn handle_client(mut stream: UnixStream, config: Arc<Mutex<Config>>) {
    // let mut reader = BufReader::new(&stream);
    // let mut line = String::new();

    let message: Vec<String> =
        bincode::decode_from_std_read(&mut stream, bincode::config::standard()).unwrap();
    // if message.len() < 1 {
    //     panic!("why len < 1 ???");
    // }
    // let command = &message[0];
    // let params = &message[1..]; /* if the vector length is 1, 1 represent the end and won't cause panic */
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
                let _ = writeln!(
                    stream,
                    "Add new App {:?}, cmd: {:?}",
                    params[0],
                    &params[1..]
                );
                let mut lock = config.lock().unwrap();
                lock.add_config(AppConfig {
                    name: params[0].to_string(),
                    path: params[1].to_string(),
                    args: params[2..].to_vec(),
                    cwd: "/".to_string(), /* TODO: implement cwd */
                    enabled: true,
                });
            } else {
                let _ = writeln!(
                    stream,
                    "usage: add <name> </path/to/app> <param1> <param2> ..."
                );
            }
        }
        "ls" => {
            let _ = writeln!(stream, "{:#?}", config.lock().unwrap());
        }
        "restart" => {
            let _ = writeln!(stream, "Restarting...");
            // TODO: Actually restart something
        }
        "enable" => {
            let _ = writeln!(stream, "Enable {}", &params[0]);
            config.lock().unwrap().enable(&params[0], true);
        }
        "disable" => {
            let _ = writeln!(stream, "Disable {}", &params[0]);
            config.lock().unwrap().enable(&params[0], false);
        }
        "quit" => {
            let _ = writeln!(stream, "Quit deamon");
            std::process::exit(0);
        }
        cmd => {
            let _ = writeln!(stream, "Unknown command: {cmd}");
        }
    }
    // if let Ok(_) = reader.read_line(&mut line) {
    //     match line.trim() {
    //         "status" => {
    //             let _ = writeln!(stream, "Daemon is running.");
    //         }
    //         "add" => {
    //             let _ = writeln!(stream, "Add new App!");
    //             let _ = writeln!(stream, "Add new App!");
    //             let mut lock = config.lock().unwrap();
    //             lock.add_config(AppConfig {
    //                 path: "fasdfas".to_string(),
    //                 args: vec![],
    //                 cwd: "".to_string(),
    //                 enabled: true,
    //             });
    //             let _ = writeln!(stream, "Add new App");
    //         }
    //         "ls" => {
    //             let _ = writeln!(stream, "{:#?}", config.lock().unwrap());
    //             // TODO: Actually restart something
    //         }
    //         "restart" => {
    //             let _ = writeln!(stream, "Restarting...");
    //             // TODO: Actually restart something
    //         }
    //         "quit" => {
    //             let _ = writeln!(stream, "Quit deamon");
    //             std::process::exit(0);
    //         }
    //         cmd => {
    //             let _ = writeln!(stream, "Unknown command: {cmd}");
    //         }
    //     }
    // }
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
