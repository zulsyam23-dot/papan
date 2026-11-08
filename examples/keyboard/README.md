# PAPAN Keyboard - Macropad 3x4

Proyek contoh **keyboard mekanikal nyata (topologi)** untuk PAPAN.

## Isi
- 12 tombol mekanikal (`switch`) dalam matrix **3 baris × 4 kolom** (grid 19.05mm)
- 12 diode (`diode`, konfigurasi col2row)
- MCU `ATmega32U4`
- Konektor `USB-C` + resistor CC (5.1k) + decoupling
- Tata letak komponen sudah diatur; **jalur PCB/ratsnest** muncul saat dibuka di Desktop (autoroute saat open jika belum ada traces)

## Buka di PAPAN Desktop
1. Jalankan app desktop (disarankan `cargo tauri dev` dari `apps/papan-desktop/src-tauri`).
2. Klik **Buka** dan masukkan path:
   `C:\Users\PC\Documents\papan\examples\keyboard`
3. Lihat ghost/ratsnest, gunakan alat Geser/Putar, `Ctrl+Z/Y`, simpan.

## Cek via CLI
```powershell
cargo run -p papan-cli -- check examples\keyboard
```

## Catatan
- Footprint/part di PAPAN library bersifat **deterministik & disederhanakan** (sesuai kemampuan proyek ini). Topologi matrix, penugasan pin, dan tata letak grid dibuat realistis; untuk fabrikasi produksi nyata, footprint mekanik/aturan manufaktur perlu divalidasi/disesuaikan lebih lanjut.
- Jalur dihasilkan oleh engine/routing (autoroute) dan ditampilkan sebagai panduan/ratsnest+traces di app.
