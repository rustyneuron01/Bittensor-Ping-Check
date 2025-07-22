#!/bin/bash

# Number of instances to run
INSTANCES=200

# Default parameters
WHITELIST="./white_list.txt"
DURATION=36000  # 10 hours
RPS=2500

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --whitelist)
            WHITELIST="$2"
            shift 2
            ;;
        --duration)
            DURATION="$2"
            shift 2
            ;;
        --rps)
            RPS="$2"
            shift 2
            ;;
        --instances)
            INSTANCES="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--whitelist FILE] [--duration SECONDS] [--rps RPS] [--instances N]"
            exit 1
            ;;
    esac
done

echo "Starting $INSTANCES instances of attack_rs..."
echo "Parameters:"
echo "  Whitelist: $WHITELIST"
echo "  Duration: $DURATION seconds"
echo "  RPS per instance: $RPS"
echo "  Total RPS: $((INSTANCES * RPS))"
echo ""

# Create a temporary directory for logs
LOG_DIR="./logs_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$LOG_DIR"

# Function to run a single instance
run_instance() {
    local instance_num=$1
    local log_file="$LOG_DIR/instance_${instance_num}.log"
    
    echo "Starting instance $instance_num..."
    cargo run --release -- \
        --whitelist "$WHITELIST" \
        --duration "$DURATION" \
        --rps "$RPS" > "$log_file" 2>&1 &
    
    echo $! > "$LOG_DIR/pid_${instance_num}"
}

# Start all instances
for i in $(seq 1 $INSTANCES); do
    run_instance $i
    sleep 0.1  # Small delay to avoid overwhelming the system
done

echo "All $INSTANCES instances started!"
echo "Log files are in: $LOG_DIR"
echo ""
echo "To monitor progress:"
echo "  tail -f $LOG_DIR/instance_*.log"
echo ""
echo "To stop all instances:"
echo "  pkill -f attack_rs"
echo ""
echo "To check running instances:"
echo "  ps aux | grep attack_rs | grep -v grep" 