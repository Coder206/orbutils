#![deny(warnings)]
#![feature(asm)]
#![feature(const_fn)]
#![cfg_attr(not(target_os = "redox"), feature(process_try_wait))]

extern crate orbclient;
extern crate orbfont;

#[cfg(not(target_os = "redox"))]
extern crate libc;

#[cfg(target_os = "redox")]
extern crate syscall;

use orbclient::event;
use std::{env, str};
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::io::{self, Result, Read, Write};
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

use console::Console;
use getpty::getpty;

mod console;
mod getpty;

#[cfg(not(target_os="redox"))]
pub fn before_exec() -> Result<()> {
    use libc;
    unsafe {
        if libc::setsid() < 0 {
            panic!("setsid: {:?}", io::Error::last_os_error());
        }
        if libc::ioctl(0, libc::TIOCSCTTY, 1) < 0 {
            panic!("ioctl: {:?}", io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(target_os="redox")]
pub fn before_exec() -> Result<()> {
    Ok(())
}

#[cfg(target_os = "redox")]
fn handle(console: &mut Console, master_fd: RawFd, process: &mut Child) {
    extern crate syscall;

    use std::os::unix::io::AsRawFd;

    let mut event_file = File::open("event:").expect("terminal: failed to open event file");

    let window_fd = console.window.as_raw_fd();
    syscall::fevent(window_fd, syscall::flag::EVENT_READ).expect("terminal: failed to fevent console window");

    let mut master = unsafe { File::from_raw_fd(master_fd) };
    syscall::fevent(master_fd, syscall::flag::EVENT_READ).expect("terminal: failed to fevent master PTY");

    let mut handle_event = |event_id: usize, event_count: usize| -> bool {
        if event_id == window_fd {
            for event in console.window.events() {
                if event.code == event::EVENT_QUIT {
                    return false;
                }

                console.input(&event);
            }

            if ! console.input.is_empty()  {
                if let Err(err) = master.write(&console.input) {
                    let term_stderr = io::stderr();
                    let mut term_stderr = term_stderr.lock();

                    let _ = term_stderr.write(b"failed to write stdin: ");
                    let _ = term_stderr.write(err.description().as_bytes());
                    let _ = term_stderr.write(b"\n");
                    return false;
                }
                let _ = master.flush();
                console.input.clear();
            }
        } else if event_id == master_fd {
            let mut packet = [0; 4096];
            let count = master.read(&mut packet).expect("terminal: failed to read master PTY");
            if count == 0 {
                if event_count == 0 {
                    return false;
                }
            } else {
                console.write(&packet[1..count], true).expect("terminal: failed to write to console");

                //if packet[0] & 1 == 1
                {
                    console.redraw();
                }
            }
        } else {
            println!("Unknown event {}", event_id);
        }

        true
    };

    handle_event(window_fd, 0);
    handle_event(master_fd, 0);

    'events: loop {
        let mut sys_event = syscall::Event::default();
        event_file.read(&mut sys_event).expect("terminal: failed to read event file");
        if ! handle_event(sys_event.id, sys_event.data) {
            break 'events;
        }
    }

    let _ = process.kill();
    process.wait().expect("terminal: failed to wait on shell");
}

#[cfg(not(target_os = "redox"))]
fn handle(console: &mut Console, master_fd: RawFd, process: &mut Child) {
    use libc;
    use std::io::ErrorKind;
    use std::thread;
    use std::time::Duration;

    unsafe {
        let size = libc::winsize {
            ws_row: console.console.h as libc::c_ushort,
            ws_col: console.console.w as libc::c_ushort,
            ws_xpixel: 0,
            ws_ypixel: 0
        };
        if libc::ioctl(master_fd, libc::TIOCSWINSZ, &size as *const libc::winsize) < 0 {
            panic!("ioctl: {:?}", io::Error::last_os_error());
        }
    }

    console.console.raw_mode = true;

    let mut master = unsafe { File::from_raw_fd(master_fd) };

    'events: loop {
        for event in console.window.events() {
            if event.code == event::EVENT_QUIT {
                break 'events;
            }

            console.input(&event);
        }

        if ! console.input.is_empty()  {
            if let Err(err) = master.write(&console.input) {
                let term_stderr = io::stderr();
                let mut term_stderr = term_stderr.lock();

                let _ = term_stderr.write(b"failed to write stdin: ");
                let _ = term_stderr.write(err.description().as_bytes());
                let _ = term_stderr.write(b"\n");
                break 'events;
            }
            let _ = master.flush();
            console.input.clear();
        }

        let mut packet = [0; 4096];
        match master.read(&mut packet) {
            Ok(0) => break 'events,
            Ok(count) => {
                console.write(&packet[..count], true).expect("terminal: failed to write to console");
                console.redraw();
            },
            Err(err) => match err.kind() {
                ErrorKind::WouldBlock => (),
                _ => panic!("terminal: failed to read master PTY: {:?}", err)
            }
        }

        match process.try_wait() {
            Ok(status) => match status {
                Some(_code) => break 'events,
                None => ()
            },
            Err(err) => match err.kind() {
                ErrorKind::WouldBlock => (),
                _ => panic!("terminal: failed to wait on child: {:?}", err)
            }
        }

        thread::sleep(Duration::new(0, 100));
    }

    let _ = process.kill();
    process.wait().expect("terminal: failed to wait on shell");
}

fn main() {
    let shell = env::args().nth(1).unwrap_or("sh".to_string());

    let (master_fd, tty_path) = getpty();

    let slave_stdin = OpenOptions::new().read(true).write(false).open(&tty_path).unwrap();
    let slave_stdout = OpenOptions::new().read(false).write(true).open(&tty_path).unwrap();
    let slave_stderr = OpenOptions::new().read(false).write(true).open(&tty_path).unwrap();

    let width = 800;
    let height = 576;

    env::set_var("COLUMNS", format!("{}", width / 8));
    env::set_var("LINES", format!("{}", height / 16));
    env::set_var("TERM", "xterm-256color");
    env::set_var("TTY", format!("{}", tty_path.display()));

    match unsafe {
        Command::new(&shell)
            .stdin(Stdio::from_raw_fd(slave_stdin.into_raw_fd()))
            .stdout(Stdio::from_raw_fd(slave_stdout.into_raw_fd()))
            .stderr(Stdio::from_raw_fd(slave_stderr.into_raw_fd()))
            .before_exec(|| {
                before_exec()
            })
            .spawn()
    } {
        Ok(mut process) => {
            let mut console = Console::new(width, height);
            handle(&mut console, master_fd, &mut process);
        },
        Err(err) => {
            let term_stderr = io::stderr();
            let mut term_stderr = term_stderr.lock();
            let _ = term_stderr.write(b"failed to execute '");
            let _ = term_stderr.write(shell.as_bytes());
            let _ = term_stderr.write(b"': ");
            let _ = term_stderr.write(err.description().as_bytes());
            let _ = term_stderr.write(b"\n");
        }
    }
}
