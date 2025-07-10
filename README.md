Quick test with Udp using the cli front:
```
# Terminal 1
cargo run -- "udp 127.0.0.1:7777" "udp 127.0.0.1:6666"
# Terminal 2
cargo run -- "udp 127.0.0.1:6666" "udp 127.0.0.1:7777"
```
