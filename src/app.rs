use crate::{
    audio::{self, Audio},
    config::{Config, Profile},
};
use std::{env, path::PathBuf};

pub fn run() -> Result<(), String> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let config_path = if args.first().is_some_and(|arg| arg == "--config") {
        if args.len() < 2 {
            return Err("--config requires a path".into());
        }
        let path = PathBuf::from(args.remove(1));
        args.remove(0);
        path
    } else {
        env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("executable has no parent directory")?
            .join("volume3.json")
    };
    if args.is_empty() || args[0] == "run" {
        return crate::ui::run_gui(config_path);
    }
    command(&config_path, &args)
}

fn command(path: &PathBuf, args: &[String]) -> Result<(), String> {
    let mut config = Config::load(path)?;
    let op = args[0].as_str();
    if op == "help" || op == "--help" {
        print_help();
        return Ok(());
    }
    if op == "profile" && args.get(1).is_some_and(|v| v == "list") {
        for name in config.profiles.keys() {
            println!(
                "{}{}",
                if *name == config.active_profile {
                    "* "
                } else {
                    "  "
                },
                name
            );
        }
        return Ok(());
    }
    let audio = Audio::new()?;
    match op {
        "profile" => profile_command(path, &mut config, &audio, args),
        "volume" => volume_command(&audio, args),
        "device" => device_command(&audio, args),
        "app" => app_command(&audio, args),
        _ => Err(format!("unknown command: {op}; run volume3 help")),
    }
}

fn profile_command(
    path: &PathBuf,
    config: &mut Config,
    audio: &Audio,
    args: &[String],
) -> Result<(), String> {
    let action = arg(args, 1, "profile action")?;
    match action {
        "use" => {
            let name = arg(args, 2, "profile name")?;
            config.switch(name)?;
            audio.apply_profile(config.active())?;
            config.save(path)?;
            println!("Active profile: {name}");
        }
        "next" | "previous" => {
            config.cycle(action == "next");
            audio.apply_profile(config.active())?;
            config.save(path)?;
            println!("Active profile: {}", config.active_profile);
        }
        "create" => {
            let name = arg(args, 2, "profile name")?;
            if config.profiles.contains_key(name) {
                return Err(format!("profile exists: {name}"));
            }
            config.profiles.insert(name.into(), snapshot(audio)?);
            config.save(path)?;
            println!("Created profile: {name}");
        }
        "save" => {
            let name = args.get(2).unwrap_or(&config.active_profile).clone();
            if !config.profiles.contains_key(&name) {
                return Err(format!("profile not found: {name}"));
            }
            config.profiles.insert(name.clone(), snapshot(audio)?);
            config.save(path)?;
            println!("Saved profile: {name}");
        }
        "delete" => {
            let name = arg(args, 2, "profile name")?;
            if config.profiles.len() == 1 {
                return Err("cannot delete the last profile".into());
            }
            if config.profiles.remove(name).is_none() {
                return Err(format!("profile not found: {name}"));
            }
            if config.active_profile == name {
                config.active_profile = config.profiles.keys().next().unwrap().clone();
            }
            config.save(path)?;
            println!("Deleted profile: {name}");
        }
        "set-device" => {
            let id = arg(args, 2, "device ID")?;
            if !audio
                .devices(audio::output_flow())?
                .iter()
                .any(|device| device.id == id)
            {
                return Err(format!("active output device not found: {id}"));
            }
            config
                .profiles
                .get_mut(&config.active_profile)
                .unwrap()
                .output_device = Some(id.into());
            config.save(path)?;
        }
        "set-device-volume" => {
            let id = arg(args, 2, "device ID")?;
            let level = parse_percent(arg(args, 3, "volume")?)?;
            config
                .profiles
                .get_mut(&config.active_profile)
                .unwrap()
                .device_volumes
                .insert(id.into(), level);
            config.save(path)?;
        }
        "set-app-volume" => {
            let name = arg(args, 2, "executable name")?;
            let level = parse_percent(arg(args, 3, "volume")?)?;
            config
                .profiles
                .get_mut(&config.active_profile)
                .unwrap()
                .application_volumes
                .insert(name.to_ascii_lowercase(), level);
            config.save(path)?;
        }
        _ => return Err(format!("unknown profile action: {action}")),
    }
    Ok(())
}

fn snapshot(audio: &Audio) -> Result<Profile, String> {
    let mut profile = Profile::default();
    profile.output_device = Some(audio.current_device_id(audio::output_flow())?);
    for device in audio.devices(audio::output_flow())? {
        if let Ok(state) = audio.state(Some(&device.id), audio::output_flow()) {
            profile.device_volumes.insert(device.id, state.percent);
        }
    }
    for session in audio.app_sessions()? {
        profile
            .application_volumes
            .insert(session.process.to_ascii_lowercase(), session.volume);
    }
    Ok(profile)
}

fn volume_command(audio: &Audio, args: &[String]) -> Result<(), String> {
    let action = arg(args, 1, "volume action")?;
    match action {
        "get" => {
            let state = audio.state(None, audio::output_flow())?;
            println!(
                "{}% {}  L:{}% R:{}% peak:{:.0}%",
                state.percent,
                if state.muted { "muted" } else { "on" },
                state.left,
                state.right,
                state.peak * 100.0
            );
        }
        "set" => audio.set_volume(
            None,
            audio::output_flow(),
            parse_percent(arg(args, 2, "volume")?)?,
        )?,
        "up" | "down" => {
            let step = args
                .get(2)
                .map(|value| parse_percent(value))
                .transpose()?
                .unwrap_or(2);
            let delta = if action == "up" {
                step as i16
            } else {
                -(step as i16)
            };
            println!("{}%", audio.change_volume(delta)?);
        }
        "mute" => audio.set_mute(None, audio::output_flow(), true)?,
        "unmute" => audio.set_mute(None, audio::output_flow(), false)?,
        "toggle" => {
            println!(
                "{}",
                if audio.toggle_mute(audio::output_flow())? {
                    "muted"
                } else {
                    "on"
                }
            );
        }
        "balance" => {
            let left = parse_percent(arg(args, 2, "left")?)?;
            let right = parse_percent(arg(args, 3, "right")?)?;
            audio.set_balance(left, right)?;
        }
        _ => return Err(format!("unknown volume action: {action}")),
    }
    Ok(())
}

fn device_command(audio: &Audio, args: &[String]) -> Result<(), String> {
    let action = arg(args, 1, "device action")?;
    let flow = if args.iter().any(|arg| arg == "input") {
        audio::input_flow()
    } else {
        audio::output_flow()
    };
    match action {
        "list" => {
            for device in audio.devices(flow)? {
                println!(
                    "{}{}\t{}",
                    if device.is_default { "* " } else { "  " },
                    device.name,
                    device.id
                );
            }
        }
        "use" => audio.set_default_device(arg(args, 2, "device ID")?, flow)?,
        "volume" => {
            let id = arg(args, 2, "device ID")?;
            let level = parse_percent(arg(args, 3, "volume")?)?;
            audio.set_volume(Some(id), flow, level)?;
        }
        "mute" | "unmute" => {
            audio.set_mute(Some(arg(args, 2, "device ID")?), flow, action == "mute")?;
        }
        _ => return Err(format!("unknown device action: {action}")),
    }
    Ok(())
}

fn app_command(audio: &Audio, args: &[String]) -> Result<(), String> {
    let action = arg(args, 1, "app action")?;
    match action {
        "list" => {
            for session in audio.app_sessions()? {
                println!(
                    "{}\t{}%\t{}\t{}",
                    session.process,
                    session.volume,
                    if session.muted { "muted" } else { "on" },
                    session.device_id
                );
            }
        }
        "set" => {
            let changed = audio.set_app_volume(
                arg(args, 2, "executable name")?,
                parse_percent(arg(args, 3, "volume")?)?,
            )?;
            println!("{changed} sessions changed");
        }
        "mute" | "unmute" => {
            let changed = audio.set_app_mute(arg(args, 2, "executable name")?, action == "mute")?;
            println!("{changed} sessions changed");
        }
        _ => return Err(format!("unknown app action: {action}")),
    }
    Ok(())
}

fn arg<'a>(args: &'a [String], index: usize, label: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("missing {label}"))
}

fn parse_percent(value: &str) -> Result<u8, String> {
    let percent = value
        .parse::<u8>()
        .map_err(|_| format!("invalid percent: {value}"))?;
    if percent > 100 {
        return Err("percent must be 0..100".into());
    }
    Ok(percent)
}

fn print_help() {
    println!("Volume3 — portable Windows 11 volume control");
    println!("volume3 [--config PATH] run");
    println!("volume3 profile list|create NAME|use NAME|next|previous|save [NAME]|delete NAME");
    println!("volume3 profile set-device ID|set-device-volume ID 0..100|set-app-volume EXE 0..100");
    println!("volume3 volume get|set N|up [N]|down [N]|mute|unmute|toggle|balance LEFT RIGHT");
    println!("volume3 device list [input]|use ID [input]|volume ID N [input]|mute ID [input]|unmute ID [input]");
    println!("volume3 app list|set EXE N|mute EXE|unmute EXE");
    println!("Settings are stored in volume3.json beside the executable unless --config is given.");
}
