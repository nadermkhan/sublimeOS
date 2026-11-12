# Broadcom BCM4331 (b43) Proprietary Microcode Directory

CosmicOS's [`b43.rs`](../../src/drivers/net/broadcom/b43.rs) wireless driver implements the genuine Broadcom BCMA backplane initialization, PSM microcode SRAM loader (`B43_MMIO_SHM_CONTROL` / `B43_MMIO_SHM_DATA`), and 64-bit DMA ring setup for **BCM4331 (`14E4:4331`)** found on Apple MacBook Pro (Late 2011) and Mac mini models.

Because Broadcom's BCM4331 uCode microcode binaries are proprietary and cannot be redistributed in-tree, `build.rs` automatically detects extracted `b43-fwcutter` binaries here and compiles them into the kernel via `include_bytes!`:

- `ucode16_mimo.fw` *(preferred for BCM4331 N-PHY MIMO; `ucode16.fw` also accepted as fallback)*
- `pcm16.fw` *(optional PCM payload for BCM4331 hardware audio/timing tables)*

### How to Extract Using `b43-fwcutter`
1. Download `broadcom-wl` (e.g. `broadcom-wl-5.100.138.tar.bz2` / Linux `firmware-b43-installer` source).
2. Run `b43-fwcutter -w ./firmware broadcom-wl-*/linux/wl_apsta.o`.
3. Copy `ucode16_mimo.fw` and `pcm16.fw` into `firmware/b43/`.
4. Rebuild CosmicOS (`.\build.ps1`). `build.rs` will automatically activate embedded firmware ingestion and microcode upload at boot.
