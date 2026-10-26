//! Which device "default" is (#203).
//!
//! A latency is kept per device pair, so the pair has to be named by what
//! it is. cpal's ALSA host names the default devices `default` whatever
//! they are routed to (`host/alsa/enumerate.rs`: `default_input_device`
//! and `default_output_device` are both a `Device` called `"default"`), and
//! lists `pulse` and `pipewire` for the sound servers' own plugins. Keyed
//! by those names, every Linux machine has one pair, `default|default`,
//! and the speakers' latency would be applied to a Bluetooth headset the
//! moment it became the default.
//!
//! So on ALSA a name that stands for "whatever the sound server routes
//! to" is replaced by the server's own name for the default sink or
//! source, read with `pactl get-default-sink` / `get-default-source`
//! (PipeWire answers the same commands through its PulseAudio server). The
//! alternative, libpulse, is a native library and a new dependency for two
//! questions a command answers; `pactl` is run with a timeout and an
//! answer that is not there is a refusal, not a guess:
//! [`RecorderError::DeviceUnidentified`], which the caller shows as the
//! reason nothing can be measured or stored. A name that does name a
//! device (`hw:CARD=PCH,DEV=0`, `sysdefault:CARD=PCH`) is kept as it is.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::RecorderError;

/// The cpal host these names belong to.
const ALSA: &str = "ALSA";
/// Names ALSA gives that route to whatever the sound server is set to.
const INDIRECT: [&str; 3] = ["default", "pulse", "pipewire"];
/// How long `pactl` may take. It answers in milliseconds when a server is
/// there; a hung socket must not hang Settings.
const PACTL_TIMEOUT: Duration = Duration::from_secs(2);

/// Which side of the pair a name is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Output,
    Input,
}

impl Role {
    fn word(self) -> &'static str {
        match self {
            Role::Output => "speakers",
            Role::Input => "microphone",
        }
    }

    fn question(self) -> &'static str {
        match self {
            Role::Output => "get-default-sink",
            Role::Input => "get-default-source",
        }
    }
}

/// Whether `host`'s `name` is a route to a default rather than a device.
pub fn is_indirect(host: &str, name: &str) -> bool {
    host == ALSA && INDIRECT.contains(&name)
}

/// `name` as it should be filed: itself when it names a device, and
/// `name (sink)` when it stands for the sound server's default. `ask` runs
/// `pactl` with the given arguments and answers its output.
pub fn identify(
    host: &str,
    name: &str,
    role: Role,
    ask: &dyn Fn(&[&str]) -> Result<String, String>,
) -> Result<String, RecorderError> {
    if !is_indirect(host, name) {
        return Ok(name.to_string());
    }
    let unidentified = |why: &str| RecorderError::DeviceUnidentified {
        role: role.word(),
        name: name.to_string(),
        why: why.to_string(),
    };
    let answer = ask(&[role.question()]).map_err(|e| unidentified(&e))?;
    let real = answer.trim();
    if real.is_empty() || real.contains('\n') {
        return Err(unidentified("the sound server named no default"));
    }
    Ok(format!("{name} ({real})"))
}

/// `pactl` with `args`, for [`identify`].
pub fn pactl(args: &[&str]) -> Result<String, String> {
    run_with_timeout("pactl", args, PACTL_TIMEOUT)
}

/// Run `program`, give up on it after `limit` (killing it), and answer
/// what it printed. A program that is not there, that fails, or that is
/// too slow is an `Err` in words.
pub fn run_with_timeout(program: &str, args: &[&str], limit: Duration) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!("{program} is not installed")
            } else {
                format!("{program} could not be started: {e}")
            }
        })?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} did not answer in {} s", limit.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(format!("{program} could not be waited for: {e}")),
        }
    };
    let read = |pipe: Option<&mut dyn Read>| {
        let mut text = String::new();
        if let Some(p) = pipe {
            let _ = p.take(64 * 1024).read_to_string(&mut text);
        }
        text
    };
    let out = read(child.stdout.as_mut().map(|p| p as &mut dyn Read));
    if !status.success() {
        let err = read(child.stderr.as_mut().map(|p| p as &mut dyn Read));
        let line = err.lines().next().unwrap_or("").trim().to_string();
        return Err(if line.is_empty() {
            format!("{program} failed")
        } else {
            format!("{program} failed: {line}")
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PCI_SINK: &str = "alsa_output.pci-0000_00_1f.3.analog-stereo";
    const BT_SINK: &str = "bluez_output.AA_BB_CC_DD_EE_FF.1";
    const PCI_SOURCE: &str = "alsa_input.pci-0000_00_1f.3.analog-stereo";

    fn server(
        sink: &'static str,
        source: &'static str,
    ) -> impl Fn(&[&str]) -> Result<String, String> {
        move |args| match args {
            ["get-default-sink"] => Ok(format!("{sink}\n")),
            ["get-default-source"] => Ok(format!("{source}\n")),
            other => panic!("unexpected question {other:?}"),
        }
    }

    fn never(_: &[&str]) -> Result<String, String> {
        panic!("a name that names a device must not be looked up")
    }

    #[test]
    fn every_name_cpal_gives_the_default_on_alsa_is_resolved_to_the_servers_default() {
        for name in ["default", "pulse", "pipewire"] {
            let got = identify("ALSA", name, Role::Output, &server(PCI_SINK, PCI_SOURCE)).unwrap();
            assert_eq!(got, format!("{name} ({PCI_SINK})"));
        }
        let mic = identify(
            "ALSA",
            "default",
            Role::Input,
            &server(PCI_SINK, PCI_SOURCE),
        )
        .unwrap();
        assert_eq!(
            mic,
            format!("default ({PCI_SOURCE})"),
            "the source, not the sink"
        );
    }

    #[test]
    fn two_different_defaults_behind_the_name_default_are_two_pairs() {
        let speakers = identify(
            "ALSA",
            "default",
            Role::Output,
            &server(PCI_SINK, PCI_SOURCE),
        )
        .unwrap();
        let headset = identify(
            "ALSA",
            "default",
            Role::Output,
            &server(BT_SINK, PCI_SOURCE),
        )
        .unwrap();
        assert_ne!(speakers, headset);
    }

    #[test]
    fn a_name_that_names_a_device_is_kept_and_nothing_is_asked() {
        for name in [
            "hw:CARD=PCH,DEV=0",
            "sysdefault:CARD=PCH",
            "plughw:CARD=USB,DEV=0",
            "front:CARD=PCH,DEV=0",
        ] {
            assert_eq!(identify("ALSA", name, Role::Output, &never).unwrap(), name);
        }
    }

    #[test]
    fn other_hosts_name_real_devices_and_are_not_asked() {
        for host in ["CoreAudio", "WASAPI", "JACK"] {
            assert_eq!(
                identify(host, "default", Role::Input, &never).unwrap(),
                "default"
            );
        }
    }

    #[test]
    fn a_server_that_cannot_be_asked_is_a_refusal_that_says_why() {
        let e = identify("ALSA", "default", Role::Output, &|_| {
            Err("pactl is not installed".into())
        })
        .unwrap_err();
        assert!(
            matches!(e, RecorderError::DeviceUnidentified { .. }),
            "{e:?}"
        );
        let text = e.to_string();
        assert!(
            text.contains("speakers") && text.contains("pactl is not installed"),
            "{text}"
        );
        assert!(
            text.contains("not be kept") || text.contains("cannot"),
            "{text}"
        );
    }

    #[test]
    fn an_empty_or_muddled_answer_is_a_refusal_not_a_name() {
        for answer in ["", "  \n", "a\nb\n"] {
            let e =
                identify("ALSA", "default", Role::Input, &|_| Ok(answer.to_string())).unwrap_err();
            assert!(
                matches!(e, RecorderError::DeviceUnidentified { .. }),
                "{answer:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_hangs_is_stopped_at_the_limit() {
        let t = Instant::now();
        let e = run_with_timeout("sleep", &["5"], Duration::from_millis(200)).unwrap_err();
        assert!(e.contains("did not answer"), "{e}");
        assert!(
            t.elapsed() < Duration::from_secs(3),
            "waited {:?}",
            t.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_answers_is_read_and_one_that_fails_or_is_missing_is_an_error() {
        let ok = run_with_timeout("sh", &["-c", "echo hello"], Duration::from_secs(2)).unwrap();
        assert_eq!(ok.trim(), "hello");
        let failed = run_with_timeout(
            "sh",
            &["-c", "echo nope >&2; exit 3"],
            Duration::from_secs(2),
        )
        .unwrap_err();
        assert!(failed.contains("nope"), "{failed}");
        let missing =
            run_with_timeout("edytlab-no-such-program", &[], Duration::from_secs(2)).unwrap_err();
        assert!(missing.contains("not installed"), "{missing}");
    }
}
