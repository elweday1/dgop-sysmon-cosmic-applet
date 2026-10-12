// SPDX-License-Identifier: GPL-3.0-only

use serde::Deserialize;
use tokio::process::Command;

const DGOP: &str = "dgop";

#[derive(Debug, Clone, Default)]
pub struct Cursors {
    cpu: Option<String>,
    proc: Option<String>,
    net: Option<String>,
    disk: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Meta {
    pub cpu: Option<Cpu>,
    pub memory: Option<Memory>,
    #[serde(rename = "netrate")]
    pub net_rate: Option<NetRate>,
    #[serde(rename = "diskrate")]
    pub disk_rate: Option<DiskRate>,
    pub processes: Option<Vec<Process>>,
    pub system: Option<System>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Cpu {
    pub count: u32,
    pub model: String,
    pub usage: f64,
    pub frequency: f64,
    pub temperature: f64,
    #[serde(rename = "coreUsage")]
    pub core_usage: Vec<f64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Memory {
    pub total: u64,
    pub used: u64,
    #[serde(rename = "usedPercent")]
    pub used_percent: f64,
    pub available: u64,
    #[serde(rename = "swaptotal")]
    pub swap_total: u64,
    #[serde(rename = "swapfree")]
    pub swap_free: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NetRate {
    pub interfaces: Vec<NetRateInfo>,
    pub cursor: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NetRateInfo {
    pub interface: String,
    #[serde(rename = "rxrate")]
    pub rx_rate: f64,
    #[serde(rename = "txrate")]
    pub tx_rate: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DiskRate {
    pub disks: Vec<DiskRateInfo>,
    pub cursor: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DiskRateInfo {
    pub device: String,
    #[serde(rename = "readrate")]
    pub read_rate: f64,
    #[serde(rename = "writerate")]
    pub write_rate: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Process {
    pub pid: i32,
    pub ppid: i32,
    pub cpu: f64,
    #[serde(rename = "memoryPercent")]
    pub memory_percent: f32,
    #[serde(rename = "memoryKB")]
    pub memory_kb: u64,
    pub username: String,
    pub command: String,
    #[serde(rename = "fullCommand")]
    pub full_command: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct System {
    #[serde(rename = "loadavg")]
    pub load_avg: String,
    pub processes: u32,
    pub threads: u32,
    pub boottime: String,
}

async fn run<T: serde::de::DeserializeOwned>(args: &[String]) -> Result<T, String> {
    let output = Command::new(DGOP)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|err| format!("failed to run {DGOP}: {err}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }

    serde_json::from_slice(&output.stdout).map_err(|err| format!("invalid {DGOP} output: {err}"))
}

pub async fn poll(full: bool, cursors: &mut Cursors) -> Result<Meta, String> {
    let mut modules = vec!["cpu", "memory", "net-rate", "disk-rate", "system"];
    if full {
        modules.push("processes");
    }

    let mut args = vec![
        "meta".to_owned(),
        "--modules".to_owned(),
        modules.join(","),
        "--sort".to_owned(),
        "cpu".to_owned(),
        "--limit".to_owned(),
        "0".to_owned(),
        "--json".to_owned(),
    ];

    for (flag, cursor) in [
        ("--cpu-cursor", &cursors.cpu),
        ("--proc-cursor", &cursors.proc),
        ("--net-rate-cursor", &cursors.net),
        ("--disk-rate-cursor", &cursors.disk),
    ] {
        if let Some(cursor) = cursor {
            args.push(flag.to_owned());
            args.push(cursor.clone());
        }
    }

    let meta: Meta = run(&args).await?;

    if let Some(cursor) = meta.cpu.as_ref().and_then(|cpu| cpu.cursor.clone()) {
        cursors.cpu = Some(cursor);
    }
    if let Some(cursor) = meta.cursor.clone().filter(|c| !c.is_empty()) {
        cursors.proc = Some(cursor);
    }
    if let Some(net) = meta.net_rate.as_ref().filter(|n| !n.cursor.is_empty()) {
        cursors.net = Some(net.cursor.clone());
    }
    if let Some(disk) = meta.disk_rate.as_ref().filter(|d| !d.cursor.is_empty()) {
        cursors.disk = Some(disk.cursor.clone());
    }

    Ok(meta)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcSignal {
    Terminate,
    Kill,
    Stop,
    Continue,
}

pub fn signal(pid: i32, signal: ProcSignal) -> Result<(), String> {
    use rustix::process::{Pid, Signal, kill_process};

    let pid = Pid::from_raw(pid).ok_or_else(|| format!("invalid pid {pid}"))?;
    let signal = match signal {
        ProcSignal::Terminate => Signal::TERM,
        ProcSignal::Kill => Signal::KILL,
        ProcSignal::Stop => Signal::STOP,
        ProcSignal::Continue => Signal::CONT,
    };
    kill_process(pid, signal).map_err(|err| err.to_string())
}
