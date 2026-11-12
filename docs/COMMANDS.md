# SublimeSH Command Reference & Shell Manual

`SublimeSH` is the native interactive shell for SublimeOS. Departing from archaic, cryptic 1970s Unix abbreviations (`cat`, `wc`, `ps`, `df`, `rm`), SublimeSH introduces a **modern, understandable, domain-oriented command architecture** while maintaining full backwards compatibility with classic Unix utilities.

SublimeSH is accessible across all virtual consoles (`Alt+F1` through `Alt+F4`), COM1 serial (`115200 8-N-1`), and the graphical desktop terminal emulator (`Terminal.app`).

---

## 1. Shell Design & Modern Philosophy

Modern systems should be self-descriptive and intuitive. SublimeSH structures commands around clear **Domains** (`system`, `tasks`, `storage`, `network`, `audio`) and **Action Verbs** (`list`, `view`, `edit`, `create`, `delete`, `find`, `count`):

- **Self-Explanatory Verbs**: Instead of guessing what `cat` or `rm` or `wc` mean, use `view`, `delete`, `count`, and `list`.
- **Hierarchical Domain Namespaces**: Manage network interfaces, Wi-Fi, DNS, and server daemons consistently under `network` (or shortcut `net`), processes under `tasks`, storage devices under `storage`, and system telemetry under `system`.
- **Zero Breaking Changes**: Classic Unix commands (`ls`, `cat`, `ps`, `top`, `grep`, `wc`, `head`, `tail`, `touch`, `mkdir`, `rm`, `ping`, `curl`, `ifconfig`, `nano`, `reboot`, etc.) remain fully functional as aliases for existing scripts, tests, and developer muscle memory.
- **Expressive Stream Pipelines (`|`)**: Modern stream processing verbs (`find`, `count`, `first`, `last`, `sort`, `unique`) integrate seamlessly into pipelines and file redirections.

---

## 2. Modern Command Palette

### System & Hardware (`system`, `sys`)
| Modern Command | Shortcut | Description | Classic Alias |
| :--- | :--- | :--- | :--- |
| `system info` | `sys` | Modern visual status dashboard (OS, Kernel, CPU, RAM gauge, Uptime, RTC) | `sw_vers`, `uname` |
| `system hardware` | `sys hw` | Comprehensive hardware profiler (model, CPU architecture, UUID) | `system_profiler` |
| `system config [key]` | `sys config` | Inspect kernel sysctl variables (`hw.ncpu`, `hw.memsize`, `hw.model`) | `sysctl [-a]` |
| `system version` | `sys vers` | Print product name, version, and build identifier | `sw_vers` |
| `system uptime` | `uptime` | Display elapsed system uptime and timer ticks | `uptime` |
| `system date` | `date` | Output current UTC date and time synchronized via RTC / NTP | `date` |
| `system memory` | `mem` | Physical frame allocator stats and KiteKernel heap metrics | `mem` |
| `system restart` | `reboot` | Perform ACPI hardware power reset | `reboot` |
| `system shutdown` | `halt` | Cleanly halt and power down system | `halt` |

### Tasks & Processes (`tasks`, `process`, `proc`)
| Modern Command | Shortcut | Description | Classic Alias |
| :--- | :--- | :--- | :--- |
| `tasks list` | `tasks`, `ps` | Display active scheduler tasks, states (Running, Ready, Sleeping), and ticks | `ps` |
| `tasks top` | `tasks monitor` | Interactive real-time process monitor with CPU and memory pressure | `top` |
| `tasks run <binary>` | `exec <binary>` | Execute 64-bit ELF binary in isolated Ring 3 user space | `exec` |
| `tasks kill <pid>` | `kill <pid>` | Deliver termination signal (`SIGKILL`) to task | `kill` |
| `tasks jobs` | `jobs` | List active, suspended, and background jobs | `jobs` |
| `fg [%n]` | `fg` | Bring job to foreground with `SIGCONT` | `fg` |
| `bg [%n]` | `bg` | Resume suspended job in background with `SIGCONT` | `bg` |

### Storage & Filesystems (`storage`, `disks`)
| Modern Command | Shortcut | Description | Classic Alias |
| :--- | :--- | :--- | :--- |
| `storage list` | `disks`, `storage` | Inspect storage controllers, block devices, and GPT/MBR partition tables | `diskutil list` |
| `storage mounts` | `mount` | Display currently mounted filesystems (RamFS, DevFS, ProcFS, FAT32, etc.) | `mount` |
| `storage cache` | `storage cache` | Inspect 512 KiB LRU Block Cache statistics (hits, misses, cached sectors) | `bcache` |
| `storage sync` | `sync` | Flush all dirty sector cache buffers out to underlying physical block devices | `sync` |
| `list [path]` | `ls` | List directory contents with file types (`[DIR]`, `[FILE]`, `[DEV]`) | `ls` |
| `view <file>` | `read`, `cat` | Output contents of a file to terminal or pipeline | `cat` |
| `edit <file>` | `nano` | In-terminal full-screen text editor (`Ctrl+S` save, `Ctrl+X` exit) | `nano` |
| `create <file>` | `touch` | Create a new empty file | `touch` |
| `delete <file>` | `remove`, `rm` | Remove a file from the filesystem | `rm` |
| `folder create <path>`| `make folder` | Create a new directory within the VFS hierarchy | `mkdir` |

### Network & Connectivity (`network`, `net`)
| Modern Command | Shortcut | Description | Classic Alias |
| :--- | :--- | :--- | :--- |
| `network status` | `net`, `ifconfig` | Show network interfaces (`lo0`, `en0`, `wlan0`), IP configs, and link states | `ifconfig` |
| `network ping <host>` | `ping <host>` | Send ICMP ECHO_REQUEST packets to network destination | `ping` |
| `network resolve <h>` | `nslookup <h>` | Resolve domain name to IPv4 address via DNS | `nslookup` |
| `network fetch <url>` | `curl [-i] <url>` | Fetch web pages & REST APIs over HTTP or secure TLS 1.3 | `curl`, `http` |
| `network wifi scan` | `wifi scan` | Scan 2.4/5 GHz spectrum for 802.11 beacons and SSIDs | `wifi scan` |
| `network wifi connect`| `wifi connect` | Authenticate to WPA2-PSK network via 4-way handshake | `wifi connect` |
| `network lease [act]` | `dhcp [status]` | DHCP client manager (`status`, `discover`, `release`) | `dhcp` |
| `network sync [srv]` | `ntp [sync]` | NTP time synchronization daemon (`status`, `sync`, `set`) | `ntp` |
| `network server <srv>`| `httpd`, `sshd` | Manage daemon services (`http`, `telnet`, `ssh`) | `httpd`, `telnetd` |

### Audio & Sound Subsystem (`audio`, `sound`)
| Modern Command | Description | Classic Alias |
| :--- | :--- | :--- |
| `audio chime` | Play iconic SublimeOS Sublime startup chime (harmonic F-Major chord) | `chime` |
| `audio tone [freq] [ms]` | Play pure sine wave audio tone via Intel HDA / PC Speaker | `beep` |
| `audio play <file.wav>` | Stream uncompressed RIFF/WAVE PCM audio file via DMA | `play` |
| `audio speak <text>` | Phonetic speech synthesizer simulated voice audio | `say` |

### Stream Processing & Filters (Pipelines)
| Modern Filter | Description | Classic Alias |
| :--- | :--- | :--- |
| `find <pattern>` | Filter stream lines matching text pattern (supports `-i` case-insensitivity, `-v` invert) | `grep` |
| `count [-l\|-w\|-c]` | Count lines (`-l`), words (`-w`), or bytes (`-c`) in stream | `wc` |
| `first [-n N]` | Extract the first N lines of stream input | `head` |
| `last [-n N]` | Extract the last N lines of stream input | `tail` |
| `sort [-r]` | Sort lines alphabetically or reversed (`-r`) | `sort` |
| `unique` | Filter duplicate adjacent lines from sorted stream | `uniq` |

---

## 3. Practical Usage Examples

### Pipeline & Stream Filtering
```bash
# View file and filter matching lines using modern verbs:
view /etc/hosts | find localhost

# Count lines matching a pattern and save to a file:
view /tmp/log.txt | find error | count -l > /tmp/error_count.txt

# Extract top 5 sorted entries:
view /etc/services | sort | first -n 5

# Inspect network interfaces and filter active links:
network status | find active
```

### System Inspection & Power
```bash
# Display modern hardware and OS dashboard:
system info

# Inspect storage layout and mounted volumes:
storage list
storage mounts

# Ping a remote gateway and fetch a REST endpoint:
network ping 1.1.1.1
network fetch http://127.0.0.1:8080/api/status

# Restart the computer cleanly:
system restart
```

---

## 4. Cross-Reference Table (Classic vs. Modern)

For users accustomed to standard Unix or macOS commands, every familiar utility continues to function as an alias:

| Classic Unix Command | Modern SublimeOS Equivalent | Primary Function |
| :--- | :--- | :--- |
| `ls` | `list` | Directory listing |
| `cat` | `view` | File content inspection |
| `grep` | `find` | Text stream line filtering |
| `wc` | `count` | Line, word, and byte counting |
| `head` | `first` | Leading line extraction |
| `tail` | `last` | Trailing line extraction |
| `uniq` | `unique` | Duplicate elimination |
| `touch` | `create` | File creation |
| `mkdir` | `make folder` / `folder create` | Directory creation |
| `rm` | `delete` / `remove` | File deletion |
| `ps` | `tasks list` | Task table listing |
| `top` | `tasks top` | Real-time task monitor |
| `ifconfig` | `network status` | Interface configuration |
| `nslookup` | `network resolve` | DNS domain lookup |
| `ping` | `network ping` | ICMP connectivity test |
| `curl` / `http` | `network fetch` | HTTP/HTTPS GET request |
| `chime` | `audio chime` | Startup audio chime |
| `beep` | `audio tone` | Audio frequency tone |
| `play` | `audio play` | WAV file DMA playback |
| `say` | `audio speak` | Speech synthesis |
| `diskutil` | `storage list` | Disk partition manager |
| `mount` | `storage mounts` | VFS mount inspector |
| `sw_vers` / `uname` | `system info` | System version & OS info |
| `system_profiler` | `system hardware` | Detailed hardware profiling |
| `reboot` | `system restart` | ACPI power reboot |
| `halt` | `system shutdown` | ACPI system power off |
