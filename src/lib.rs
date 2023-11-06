use evdev::{Device, Key, SwitchType, InputEvent, EventType};
use tokio::io::{BufReader, AsyncBufReadExt};
use tokio::fs::{File, read_dir};
use std::collections::HashMap;
use std::sync::Arc;
use std::fmt::Write;
use tokio::process::Command;

pub async fn run() {
	let rules = Arc::new(parse_rules("/etc/ehcpi-rs.conf").await.unwrap());
	let devices = get_devices(&rules).await.unwrap();
	let mut handlers = Vec::new();
	for device in devices {
		let rules = rules.clone();
		handlers.push(tokio::spawn(async move {
			let mut stream = device.into_event_stream().unwrap();
			while let Ok(ev) = stream.next_event().await {
				if let Some(c) = get_cmd(&ev, &rules) {
					Command::new("sh")
						.arg("-c")
						.arg(c)
						.output()
						.await
						.unwrap();
				}
			}
		}));
	}
	futures::future::join_all(handlers).await;
}

fn get_cmd<'a> (event: &InputEvent, rules: &'a HashMap<EhcpiEvent, String>) 
	-> Option<&'a String> {
	let ev = match event.event_type() {
		EventType::KEY => {
			if event.value() != 1 {
				return None;
			}
			let key = Key::new(event.code());
			EhcpiEvent::Key(key)
		}
		EventType::SWITCH => {
			EhcpiEvent::Switch(event.code(), event.value())
		}
		_ => return None,
	};
	rules.get(&ev)
}

#[derive(Eq, Hash, PartialEq)]
enum EhcpiEvent {
	Key(Key),
	Switch(u16, i32),
}

#[derive(Debug)]
enum RuleParseError {
	IoError(std::io::Error),
	ParseError,
}

impl From<std::io::Error> for RuleParseError {
	fn from(error: std::io::Error) -> Self {
		RuleParseError::IoError(error)
	}
}

async fn get_devices(rules: &HashMap<EhcpiEvent, String>) -> Result<Vec<Device>, std::io::Error> {
	let mut result = Vec::new();
	let mut dir = read_dir("/dev/input").await?;
	while let Some(entry) = dir.next_entry().await? {
		let filename = entry.path();
		let filename = filename.to_str().unwrap();
		if !filename.starts_with("/dev/input/event") {
			continue;
		}
		let dev = Device::open(filename)?;
		for rule in rules {
			match rule.0 {
				EhcpiEvent::Key(k) => {
					let keys = match dev.supported_keys() {
						Some(k) => k,
						None => continue,
					};
					if keys.contains(*k) {
						result.push(dev);
						break;
					}
				}
				EhcpiEvent::Switch(s, _) => {
					let switches = match dev.supported_switches() {
						Some(s) => s,
						None => continue,
					};
					let switch = SwitchType(*s);
					if switches.contains(switch) {
						result.push(dev);
						break;
					}
				}
			}
		}
	}
	Ok(result)
}

async fn parse_rules(path: &str) -> Result<HashMap<EhcpiEvent, String>, RuleParseError> {
	let mut ret = HashMap::new();
	let file = File::open(path).await?;
	let file = BufReader::new(file);
	let mut lines = file.lines();
	while let Some(line) = lines.next_line().await? {
		let mut iter = line.split_ascii_whitespace();
		let key = iter.next().ok_or(RuleParseError::ParseError)?;
		let event: EhcpiEvent;
		if key.starts_with("KEY") {
			event = EhcpiEvent::Key(key.parse().map_err(|_| RuleParseError::ParseError)?);
		}
		else if key.starts_with("SW") {
			let switch: SwitchType = key.parse().map_err(|_| RuleParseError::ParseError)?;
			let val = iter.next().ok_or(RuleParseError::ParseError)?;
			let val = val.parse().map_err(|_| RuleParseError::ParseError)?;
			event = EhcpiEvent::Switch(switch.0, val);
		}
		else {
			return Err(RuleParseError::ParseError);
		}
		let syn = iter.next().ok_or(RuleParseError::ParseError)?;
		if syn != "do" {
			return Err(RuleParseError::ParseError);
		}
		let mut cmd = String::new();
		for c in iter {
			write!(cmd, "{c} ").unwrap();
		}
		if cmd.is_empty() {
			return Err(RuleParseError::ParseError);
		}
		//let cmd = iter.next().ok_or(RuleParseError::ParseError)?;
		ret.insert(event, cmd.to_string());
	}
	Ok(ret)
}