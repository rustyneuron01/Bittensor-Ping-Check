# Block Receive Ping Request
sudo iptables -A INPUT -p icmp --icmp-type echo-reply -j DROP

# Make Executable
cargo build --release
chmod +x run_multiple.sh

# Stop
pkill -f attack_rs