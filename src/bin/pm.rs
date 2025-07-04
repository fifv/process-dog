use std::ffi::OsStr;
use std::fs::{File, read_dir};
use std::path::Path;
/**
 * Design
 * - a daemon, listening on socket
 * - a cli, send to daemon via socket
 * - cli add a new process, daemon start it and add it to list
 * - daemon keep the list sync with config file
 * - if daemon start/stop, run/kill all processes on the list
 * -
 */
/**
 * Design of flow
 */
use std::process::{Command, Stdio};
use std::time;
type ProcessTable =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, std::process::Child>>>;

fn process_watchdog_loop<S: AsRef<OsStr>, P: AsRef<Path>>(
    program: S,
    log_file: P,
    process_table: &ProcessTable,
) {
    fn spawn_process<S: AsRef<OsStr>, P: AsRef<Path>>(
        program: S,
        log_file: P,
    ) -> std::io::Result<std::process::Child> {
        let log = File::create(log_file)?;

        Command::new(program)
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .spawn()
    }

    loop {
        match spawn_process(&program, &log_file) {
            Ok(child) => {
                let mut table = process_table.lock().unwrap();
                let key = program.as_ref().to_str().expect("?").to_string();
                if table.contains_key(&key) {
                    eprintln!("Process already started");
                } else {
                    table.insert(key.clone(), child);
                    let child = table.get_mut(&key);
                    if let Some(child) = child {
                        let status = child.wait().expect("Failed to wait on child");
                        eprintln!("Process exited with: {} Wait for restart...", status);
                        table.remove(&key);
                    }
                }


                std::thread::sleep(time::Duration::from_millis(2000));
                // You can add logic to limit restart attempts, delay, etc.
            }
            Err(e) => {
                eprintln!("Failed to start process: {} Wait for restart...", e);

                std::thread::sleep(time::Duration::from_millis(2000));
                // Optionally sleep before retrying
            }
        }
    }
}
fn main() {
    let process_table: ProcessTable =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    // read_dir("/mnt/UDISK").unwrap().for_each(|entry| println!("{entry:?}"));
    process_watchdog_loop("/mnt/UDISK/try-mpp-vi2vo", "/tmp/myapp.log", &process_table);
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
