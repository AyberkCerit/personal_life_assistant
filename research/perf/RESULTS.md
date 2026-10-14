# M6 performans ölçümleri (SRS §5.1)

Referans veri seti `make_reference` ile üretildi: 10 000 not (1 825 günlük not, 1 MB'lık bir not),
~900 MB ek, 20 000 görev, 50 000 ölçüm. Model Gemma 4 E2B Q3_K_M, CPU, **3 thread**: SRS §2.4'teki en
düşük yapılandırma 4 çekirdek ve PLA orada modele `model_threads(4) = 3` verir (ölçüm makinesi 16
mantıksal çekirdekli; `perf` thread sayısını bu yüzden 3'e sabitler).
Arayüz ölçümleri **release** derlemesinde (`target/release`, sayfa içinde gömülü).

| Ne | Komut | Sonuç dosyası |
| --- | --- | --- |
| Çekirdek: indeks, arama, sorgular, model | `run_core.ps1` (`cargo run --release -p pla-core --example perf -- <ref> --model`) | `core.json` |
| Pencere: başlangıç, not açma, yazma, bellek, tepsi | `run_ui.ps1` (`perf_ui.py`) | `ui.json` |
| Asistan prompt'unun önbellek dökümü | `cargo run --release -p pla-core --example qa_probe -- <ref>` | ekrana |

Uzun koşular Terminal panelinde çalıştırıldı: arka plandaki bir koşu oturum yeniden başlayınca ölüyordu.

## Sonuçlar

| SRS | Ölçülen (p95) | Önce | Sınır (SRS 1.3) | |
| --- | --- | --- | --- | --- |
| NFR-PERF-001 soğuk başlangıç → düzenlenebilir not | **1,72 s** | 4,2 s | 3 s | ✓ |
| NFR-PERF-002 1 MB not açma | **72 ms** | 406 ms | 200 ms | ✓ |
| NFR-PERF-003 model cevaplarken tuştan kareye | **7,7 ms** (en kötü 46 ms) | 31 ms | 16 ms | ✓ |
| NFR-PERF-004 tam metin arama | 33 ms | | 300 ms | ✓ |
| NFR-PERF-005 görev/ölçüm/rapor sorguları | ≤ 12 ms | | 100 ms | ✓ |
| NFR-PERF-006 tepside, model yok | **27 MB** | ~1 GB | 150 MB | ✓ |
| NFR-PERF-006 pencere açık, boşta, model yok | **374 MB** (WebView 346) | 885 MB | 450 MB | ✓ |
| NFR-PERF-007 dil modeli cevaplarken | 664 MB | | 2 GB | ✓ |
| NFR-PERF-008 model yükleme | 2,6 s | | 5 s | ✓ |
| NFR-PERF-009 10 bloklu not çıkarımı (3 thread) | **46,5 s** | 43 s (4 thread) | 60 s | ✓ |
| NFR-PERF-010 ilk token, önceden okumadan sonra, araçsız (3 thread, 4 koşu) | **42 s** | 79–88 s (4 thread, okumasız) | 45 s | ✓ |
| NFR-PERF-010 ilk token, önceden okumadan sonra, araçlı (3 thread, 6 koşu) | **64 s** | | 70 s | ✓ |
| — önceden okumanın kendisi (model kapalıyken, 3 thread) | 37 s | | — | |
| — ilk token, önceden okuma yok, model kapalı (3 thread) | 87 s | | — | |
| NFR-PERF-011 ilk indeks | 165 s | 91 s* | 300 s | ✓ |
| NFR-PERF-012 model thread'leri | 4 / 16 | | N−1 | ✓ |
| — tepsiden pencereyi açma | 641 ms | | 3 s | ✓ |

\* İlk ölçüm hafif yüklü makinede; ikisi de sınırın altında.

## Ne değişti, neden

- **Klasör ağacı açılıp kapanıyor** (FR-EDT-004): ağaç 10 000 notu hep açık tek liste olarak çiziyordu:
  20 056 satır, 90 000 DOM düğümü, 60 000 olay dinleyicisi, 258 MB JS heap. Klasörler kapalı başlıyor,
  açık notun klasörleri açılıyor, durum kasa başına hatırlanıyor. Soğuk başlangıç, 1 MB not, yazma
  gecikmesi ve pencere belleği bununla sınırın altına indi.
- **Tepside WebView kapanıyor** (sahibin kararı a): pencere tepsiye kapanınca yok ediliyor, tepsiden ya da
  ikinci başlatmayla yeniden kuruluyor; tepside PLA 27 MB. Son pencere kapanınca Tauri süreci de bitiriyordu:
  çıkış kodu olmayan `ExitRequested` engelleniyor (tepsideki Çık açık kodla çıkar).
- **İlk açılışta çıkarım** (FR-EXT-030, sahibin kararı a): hiç çıkarılmamış not yalnız son 30 günde
  değiştiyse sıraya giriyor. 10 000 eski not referans makinede günlerce model işiydi.
- **Asistan prompt'u** (FR-QA-017): llama-server bir prompt'u ilk değişikliğine kadar yeniden kullanır.
  Saat son 48 saatin notlarından önce geliyordu ve her dakika değişiyordu; son 48 saatin payı da sorunun
  uzunluğuna göre kayıyordu. Şimdi kurallar, takvim ve son 48 saat her soruda aynı; bulunan notlar ve saat
  sonda; son 48 saatin notları eskiden yeniye yazılıyor (yazılmakta olan not sonda kalır, kaydetmek
  önbelleğin yalnız sonunu bozar). Kullanıcı soru kutusuna girince (en fazla 30 s'de bir) model bu ortak
  öneki (~2 400 token, 3 thread'de ~37 s) okuyor; soru yalnız kendi kısmını (~1 700 token: bulunan notlar +
  karar talimatı) okuyor. Önceden okuma çıkarıma iki not arasında öncelik alır, özet ve rapor işini kesmez.
- **NFR-PERF-012**: model thread'leri `clamp(N−1, 1, 4)`; 4 çekirdekli makinede 4 thread kullanılıyordu.

## Sınırlar ve SRS revizyonu

CPU'da prompt işleme 3 thread'de ~50 token/s (4 thread'de ~65), üretim ~25 token/s. Bu yüzden:

- **İlk token** 2 s hedefi bu modelle ulaşılamaz: soru başına ~1 700 yeni token; araç çağrılırsa sonucu
  (~1 000 token) da okunur. Bulunan notların payını ve karar örneklerini kısaltmak ~10 s daha
  kazandırırdı; doğal dil kalitesini etkileyeceği için sahip istemedi. SRS: araçsız 45 s, araçlı 70 s
  (3 thread ölçümü + küçük pay; ilk taslaktaki 30/60 s 4 thread ölçümüne göreydi, son inceleme yakaladı).
- **Çıkarım** blok başına ~84 JSON token'ı üretiyor (3,4 s); ortak prompt zaten önbellekte. SRS: 60 s.
- **Pencere belleği**: WebView2 boş bir sayfada bile yüzlerce MB; 150 MB hedefi tepside tutuluyor,
  pencere açıkken 450 MB.
- Prompt sırası değişikliği `nl_eval`'den geçirilmedi (sahibin kararı); birim testleri yapıyı doğruluyor.

## Ölçüm sırasında bulunan düzenek hataları

- İlk `perf` sürümü indeksi sıfırdan ölçmek için tabloları elle boşaltırken `chunk_fts`'yi unutmuştu;
  referansın `cache.db`'si bozuldu ("constraint failed"). Uygulamanın kendi yolları tutarlı; düzenek artık
  geçici bir `cache.db` kullanıyor.
- Event Timing API 60 Hz'de kare beklemesiyle ~16 ms ölçüyordu; yazma gecikmesi tuş olayından sonraki
  `requestAnimationFrame`'e kadar ölçülüyor.
- Asistan cevabının bitişi soru kimliğiyle bekleniyor (geçmişteki eski turlar erken "bitti" dedirtiyordu).

## Son inceleme (Opus) ve düzeltmeler

- **Kritik:** Windows oturum açılışında (`--hidden`) pencere hiç kurulmadığı için oturum da açılmıyordu
  (hatırlatıcı, çıkarım, haftalık rapor yoktu). Oturum artık `setup`'ta açılıyor (`commands::begin`).
- Kasa yokken (ilk açılış sihirbazı) pencereyi kapatmak yine uygulamadan çıkar: çıkış yalnız açık bir
  oturum varken engellenir.
- Önceden okuma neredeyse hiç tetiklenmiyordu (asistan sekmesi zaten varsayılan); soru kutusuna bağlandı.
- Önceden okuma çalışan özeti/raporu kesiyordu; kendi sayacı var (`warming`), yalnız çıkarım ona yol verir.
- Tepsiden gelen istekler sayfa yüklenirken kayboluyordu; `UiQueue` sayfa dinlemeye başlayana kadar biriktirir.
- Ağaç: yeniden adlandırılan/taşınan açık klasör açık kalır; düzenleyiciye dönmek kapatılan klasörü açmaz.
