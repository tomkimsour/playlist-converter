use progenitor_client::{Error, ResponseValue};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PRISM: &str = "@stoplight/prism-cli@5.16.0";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);

pub struct Prism {
    pub url: String,
    child: Child,
}

impl Prism {
    /// Serves `spec` with a static Prism mock on a free local port and waits until it answers HTTP.
    pub fn start(spec: &Path) -> Prism {
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut child = Command::new("npx")
            .args([
                "-y",
                PRISM,
                "mock",
                "-h",
                "127.0.0.1",
                "-p",
                &port.to_string(),
            ])
            .arg(spec)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap_or_else(|e| panic!("cannot run `npx` (is Node.js installed?): {e}"));
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        while TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("prism exited during startup: {status}");
            }
            if Instant::now() > deadline {
                let _ = Prism {
                    url: String::new(),
                    child,
                }
                .drop_group();
                panic!(
                    "prism not ready after {STARTUP_TIMEOUT:?} for {}",
                    spec.display()
                );
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Prism {
            url: format!("http://127.0.0.1:{port}"),
            child,
        }
    }

    fn drop_group(mut self) {
        self.terminate();
    }

    fn terminate(&mut self) {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(format!("-{}", self.child.id()))
            .status();
        let _ = self.child.wait();
    }
}

impl Drop for Prism {
    fn drop(&mut self) {
        if self.child.try_wait().map(|s| s.is_none()).unwrap_or(false) {
            self.terminate();
        }
    }
}

/// Writes a copy of the vendored TIDAL spec adapted for Prism and returns its path.
pub fn tidal_prism_spec() -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("specs/tidal.json");
    let mut spec: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(src).unwrap()).unwrap();
    spec["components"]["schemas"]["Included"]["items"] = serde_json::json!({});
    add_color_example(&mut spec["components"]["schemas"]);
    for item in spec["paths"].as_object_mut().unwrap().values_mut() {
        for op in item.as_object_mut().unwrap().values_mut() {
            for p in op["parameters"].as_array_mut().into_iter().flatten() {
                if p["name"].as_str().is_some_and(|n| n.contains('[')) {
                    p["required"] = false.into();
                }
            }
        }
    }
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("tidal-prism.json");
    std::fs::write(&out, serde_json::to_vec(&spec).unwrap()).unwrap();
    out
}

/// Panics with the status and body of a non-2xx response, or the error otherwise.
pub async fn fail(name: &str, e: Error<()>) -> ! {
    match e {
        Error::UnexpectedResponse(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            panic!("{name}: {status}\n{body}")
        }
        e => panic!("{name}: {e:?}"),
    }
}

/// Unwraps a generated-client result, reporting Prism's explanation on failure.
pub async fn ok<T>(name: &str, r: Result<ResponseValue<T>, Error<()>>) -> T {
    match r {
        Ok(v) => v.into_inner(),
        Err(e) => fail(name, e).await,
    }
}

const COLOR_PATTERN: &str = "^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$";

fn add_color_example(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            if m.get("pattern").and_then(|p| p.as_str()) == Some(COLOR_PATTERN) {
                m.insert("example".into(), "#ffffff".into());
            }
            m.values_mut().for_each(add_color_example);
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(add_color_example),
        _ => {}
    }
}
