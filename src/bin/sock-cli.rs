use std::env;
use std::fs;
use std::io;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;


const SOCKET_PATH: &str = "/tmp/pm.sock";

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub path: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub enabled: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub apps: Vec<AppConfig>,
    #[serde(skip)]
    config_filepath: String,
}

impl Config {
    pub fn load<P: AsRef<std::path::Path>>(&self) -> std::io::Result<Self> {
        match fs::read_to_string(&self.config_filepath) {
            Ok(content) => {
                let config: Self = toml::from_str(&content)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                Ok(config)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // File missing: use default
                Ok(self.default())
            }
            Err(err) => Err(err), // Propagate other errors
        }

        // let content = fs::read_to_string(&self.config_filepath)?;
        // let config = toml::from_str(&content)
        //     .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        // Ok(config)
    }

    pub fn save<P: AsRef<std::path::Path>>(&self) -> std::io::Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(&self.config_filepath, content)
    }
    pub fn add_config(&mut self, config: AppConfig) {
        self.apps.push(config);
    }

    pub fn find_config(&self, path: &str) -> std::option::Option<&AppConfig> {
        self.apps.iter().find(|i|i.path == path)
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

    if command == "daemon" {
        start_daemon()
    } else {
        start_cli(command)
    }
}

fn start_cli(command: &str) -> std::io::Result<()> {
    match UnixStream::connect(SOCKET_PATH) {
        Ok(mut stream) => {
            writeln!(stream, "{command}")?;
            let mut reader = BufReader::new(stream);
            let mut response = String::new();
            reader.read_line(&mut response)?;
            println!("{}", response.trim());
        }
        Err(e) => {
            let args: Vec<String> = env::args().collect();
            eprintln!(
                "Failed to connect to daemon, make sure daemon is running. It can start with `{} daemon` <{e}>",
                &args[0]
            );
        }
    };
    Ok(())
}
fn start_listening() -> std::io::Result<()> {
    // Remove previous socket file if it exists
    let _ = fs::remove_file(SOCKET_PATH);

    let listener = UnixListener::bind(SOCKET_PATH)?;
    println!("Daemon listening on {}", SOCKET_PATH);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| handle_client(stream));
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
        .working_directory("/tmp") // for default behaviour.
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
            start_listening()?;
            Ok(())
        }
        Err(error) => {
            /* ugly hack */
            if error.to_string().contains("unable to lock pid file") {
                eprintln!("[pm][Warn] Daemon may be already started, do nothing");
                Ok(())
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

fn handle_client(mut stream: UnixStream) {
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();

    if let Ok(_) = reader.read_line(&mut line) {
        match line.trim() {
            "status" => {
                let _ = writeln!(stream, "Daemon is running.");
            }
            "restart" => {
                let _ = writeln!(stream, "Restarting...");
                // TODO: Actually restart something
            }
            "quit" => {
                let _ = writeln!(stream, "Quit deamon");
                std::process::exit(0);
            }
            cmd => {
                let _ = writeln!(stream, "Unknown command: {cmd}");
            }
        }
    }
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
