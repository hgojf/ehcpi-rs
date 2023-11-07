use evdev::{Device, Key, InputEvent, SwitchType, EventType};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::process::Command;

pub fn run() {
	let rules = parse_rules("/etc/ehcpi-rs.conf").unwrap();
	let devices = get_devices(&rules).unwrap();
	run_async(Arc::new(rules), devices);
}

#[tokio::main]
async fn run_async(rules: Arc<HashMap<EhcpiEvent, String>>, devices: Vec<Device>) {
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

#[derive(Eq, Hash, PartialEq, Debug)]
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

fn get_devices(rules: &HashMap<EhcpiEvent, String>) -> Result<Vec<Device>, std::io::Error> {
	use std::fs::read_dir;
	let mut result = Vec::new();
	let dir = read_dir("/dev/input")?;
	for entry in dir {
		let filename = entry?.path();
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

fn parse_rules(path: &str) -> Result<HashMap<EhcpiEvent, String>, RuleParseError> {
	use std::fmt::Write;
	use std::io::{BufRead, BufReader};
	use std::fs::File;
	let mut ret = HashMap::new();
	let file = File::open(path)?;
	let file = BufReader::new(file);
	let lines = file.lines();
	for line in lines {
		let line = line?;
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

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn test_rules() {
		let wanted = HashMap::from([
		(EhcpiEvent::Key(Key::KEY_MUTE), "something ".into()),
		(EhcpiEvent::Switch(SwitchType::SW_LID.0, 0), "something else ".into()),
		]);
		let rules = parse_rules("examples/ehcpi-rs.conf").unwrap();
		assert_eq!(wanted, rules);
	}
}
