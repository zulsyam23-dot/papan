PAPAN - Scrip/Proyek Uji Coba
=============================

Buka di PAPAN Desktop (aplikasi Tauri):

1. Jalankan aplikasi (jika belum):
   cd C:\Users\PC\Documents\papan\apps\papan-desktop\src-tauri
   $env:RC="C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\rc.exe"
   cargo tauri dev

2. Di aplikasi, pada field path masukkan:
   C:\Users\PC\Documents\papan\examples\ujicoba
   lalu klik Open.

3. Atau buka src\main.pn, salin isinya ke panel Source (kanan), lalu klik Check Source.

4. Hasil yang diharapkan:
   - Board 80x50mm, 2 layer tampil di kanvas.
   - 4 komponen (R1,R2,R3,C1) terpasang dengan marker/pad.
   - 4 net (VCC,GND,SIG1,SIG2) di panel Nets.
   - Diagnostics bersih (atau sesuai hasil compile engine).

Catatan: UI desktop saat ini adalah viewer/editor shell (KiCad-style).
Autoroute/DRC interaktif dari UI belum ditambahkan; bisa diuji via CLI:
  cargo run -p papan-cli -- check examples\ujicoba
  cargo run -p papan-cli -- build examples\ujicoba --target manufacturing --force
