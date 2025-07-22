use std::{
    net::{IpAddr, SocketAddrV4, Ipv4Addr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    fs,
    sync::Mutex,
    task,
    time,
};
use rand::seq::SliceRandom;
use clap::Parser;
use sysinfo::{System, SystemExt};

// Command line arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to whitelist file
    #[arg(short, long, default_value = "./white_list.txt")]
    whitelist: String,

    /// Duration in seconds
    #[arg(short, long, default_value_t = 36000)]
    duration: u64,

    /// Total requests per second
    #[arg(short, long, default_value_t = 2500)]
    rps: usize,
}

// Statistics structure
#[derive(Default)]
struct Stats {
    total_requests: usize,
    batches_sent: usize,
    start_time: Option<Instant>,
}

// Main worker structure
struct PingWorker {
    targets: Vec<IpAddr>,
    stats: Arc<Mutex<Stats>>,
}

impl PingWorker {
    fn new(targets: Vec<IpAddr>) -> Self {
        Self {
            targets,
            stats: Arc::new(Mutex::new(Stats::default())),
        }
    }

    async fn run(&self, rps: usize, duration: Duration) {
        let stats = self.stats.clone();
        let mut stats_lock = stats.lock().await;
        stats_lock.start_time = Some(Instant::now());
        drop(stats_lock);

        let batch_interval = Duration::from_secs_f64(1.0);
        let mut interval = time::interval(batch_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        let start = Instant::now();
        while start.elapsed() < duration {
            interval.tick().await;

            let batch_start = Instant::now();
            self.send_batch(rps).await;

            let mut stats = stats.lock().await;
            stats.batches_sent += 1;
            stats.total_requests += rps;

            if stats.batches_sent % 60 == 0 {
                let _elapsed = batch_start.elapsed();
                println!(
                    "Progress: {:.1}s | Batches: {} | Total Pings: {}",
                    start.elapsed().as_secs_f32(),
                    stats.batches_sent,
                    stats.total_requests
                );
            }
        }
    }

    async fn send_batch(&self, count: usize) {
        let mut tasks = Vec::with_capacity(count);

        for _ in 0..count {
            let targets = self.targets.clone();
            tasks.push(task::spawn(async move {
                // Use a simple random selection without ThreadRng
                let index = (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as usize) % targets.len();
                let target = targets[index];
                if let Err(e) = send_ping(target).await {
                    eprintln!("Error sending ping: {}", e);
                }
            }));
        }

        for task in tasks {
            let _ = task.await;
        }
    }
}

async fn send_ping(target: IpAddr) -> std::io::Result<()> {
    // On Linux we can use raw sockets for better performance
    #[cfg(target_os = "linux")]
    {
        use socket2::{Domain, Protocol, Socket, Type};
        let socket = Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4))?;
        
        // Convert IpAddr to Ipv4Addr for SocketAddrV4
        let ipv4_addr = match target {
            IpAddr::V4(addr) => addr,
            IpAddr::V6(_) => return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "IPv6 addresses not supported for raw sockets",
            )),
        };
        
        let sock_addr = SocketAddrV4::new(ipv4_addr, 0);
        socket.connect(&socket2::SockAddr::from(sock_addr))?;
        
        // Build ICMP echo request packet
        let mut packet = [0u8; 64];
        packet[0] = 8;  // ICMP Echo Request
        packet[1] = 0;  // Code
        let checksum = icmp_checksum(&packet);
        packet[2..4].copy_from_slice(&checksum.to_be_bytes());
        
        socket.send(&packet)?;
        Ok(())
    }
    
    // Fallback to system ping command on other OS
    #[cfg(not(target_os = "linux"))]
    {
        let status = if cfg!(target_os = "windows") {
            tokio::process::Command::new("ping")
                .arg("-n")
                .arg("1")
                .arg("-w")
                .arg("100")
                .arg(target.to_string())
                .status()
                .await?
        } else {
            tokio::process::Command::new("ping")
                .arg("-c")
                .arg("1")
                .arg("-W")
                .arg("0.1")
                .arg("-s")
                .arg("1032")
                .arg(target.to_string())
                .status()
                .await?
        };

        println!("Ping command status: {:?}", status);
        
        if !status.success() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Ping command failed",
            ));
        }
        Ok(())
    }
}

fn icmp_checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0;
    while i < data.len() {
        let word = if i + 1 < data.len() {
            ((data[i] as u32) << 8) | (data[i + 1] as u32)
        } else {
            (data[i] as u32) << 8
        };
        sum += word;
        i += 2;
    }
    
    while (sum >> 16) > 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    
    !sum as u16
}

async fn load_whitelist(path: &str) -> std::io::Result<Vec<IpAddr>> {
    let content = fs::read_to_string(path).await?;
    let mut ips = Vec::new();

    // Try parsing as JSON array first
    if let Ok(json_ips) = serde_json::from_str::<Vec<String>>(&content) {
        for ip in json_ips {
            if let Ok(addr) = ip.parse() {
                ips.push(addr);
            }
        }
    } else {
        // Fallback to line-separated format
        for line in content.lines() {
            let line = line.trim();
            if !line.is_empty() {
                if let Ok(addr) = line.parse() {
                    ips.push(addr);
                }
            }
        }
    }

    Ok(ips)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    // Load whitelist
    let targets = load_whitelist(&args.whitelist).await?;
    if targets.is_empty() {
        eprintln!("No valid IP addresses found in whitelist");
        return Ok(());
    }
    println!("Loaded {} target IPs", targets.len());

    // Determine optimal worker count
    let sys = System::new();
    let core_count = sys.cpus().len();
    let worker_count = core_count.max(1).min(16); // Ensure at least 1 worker
    let rps_per_worker = args.rps / worker_count;
    
    println!("Starting {} workers with {} RPS each (total: {} RPS)", 
        worker_count, rps_per_worker, rps_per_worker * worker_count);

    // Setup iptables to block replies (Linux only)
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::process::Command::new("sudo")
            .arg("iptables")
            .arg("-A")
            .arg("INPUT")
            .arg("-p")
            .arg("icmp")
            .arg("--icmp-type")
            .arg("echo-reply")
            .arg("-j")
            .arg("DROP")
            .status()
            .await?;
    }

    // Create workers
    let mut workers = Vec::new();
    for _ in 0..worker_count {
        let worker = PingWorker::new(targets.clone());
        workers.push(worker);
    }

    // Run workers
    let mut handles = Vec::new();
    for worker in workers {
        let handle = tokio::spawn(async move {
            worker.run(rps_per_worker, Duration::from_secs(args.duration)).await;
        });
        handles.push(handle);
    }

    // Wait for all workers to complete
    for handle in handles {
        handle.await?;
    }

    // Cleanup iptables (Linux only)
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::process::Command::new("sudo")
            .arg("iptables")
            .arg("-D")
            .arg("INPUT")
            .arg("-p")
            .arg("icmp")
            .arg("--icmp-type")
            .arg("echo-reply")
            .arg("-j")
            .arg("DROP")
            .status()
            .await?;
    }

    Ok(())
}