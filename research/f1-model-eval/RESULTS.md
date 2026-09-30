# F1 — Model Fizibilite Denemesi Sonuçları (2026-09-30)

**Karar: GEÇTİ.** Önerilen yapılandırma: Gemma 4 E2B-it **Q3_K_M**, düşük bellek ayarlarıyla (1,85 GB, TR %96 / EN %95 — aşağıdaki bellek bölümü). İlk ölçüm Gemma 4 E2B-it (Q4_0 QAT), talimat v2 ve şema sırası düzeltmesiyle (v3), karar kapısını (alan doğruluğu ≥ %80, not başına ≤ 10 sn) iki dilde de aştı. Kendi model eğitimine (yedek plan) şimdilik gerek yok.

## Kurulum
- llama.cpp `llama-server` b11280 (CPU x64), 4 iş parçacığı, GPU kapalı, 4096 bağlam, sıcaklık 0.
- Çıktı, SRS C.3'ten türetilen JSON şemasıyla kısıtlı (`response_format: json_schema`).
- Göreli zaman Python'da, Rust'ta uygulanacak kurallarla mutlak tarihe çevrildi (`run_eval.py` → `resolve_date`).
- Değerlendirme seti: 100 TR + 100 EN not (`dataset.py`); 40 görev, 25 metrik, 15 çoklu, 15 boş, 5 farklı referans tarihli not.
- Makine: i7-13620H (16 iş parçacığı), performans modu açık. Ölçüm 4 iş parçacığıyla sınırlandı.

## Sonuçlar (alan doğruluğu = tam doğru çıkarılan beklenen öğe oranı)

| Model | Dosya | Bellek (çalışma / özel) | Talimat | TR | EN | Kesinlik | p50 / p95 |
|---|---|---|---|---|---|---|---|
| Qwen2.5-1.5B | 1.12 GB | 1.91 / 0.98 GB | v1 | %32 | %41 | %38 | 1.3 / 2.6 sn |
| Qwen2.5-1.5B | | | v2 | %46 | %61 | %52 | 1.5 / 2.7 sn |
| Qwen3-1.7B | 1.11 GB | 2.42 / 1.37 GB | v1 | %52 | %64 | %66 | 1.5 / 2.7 sn |
| Qwen3-1.7B | | | v2 | %64 | %76 | %73 | 1.8 / 3.6 sn |
| Qwen3-4B-Instruct-2507 | 2.50 GB | 5.04 / 2.63 GB | v2 | %84 | %86 | %85 | 4.5 / 8.4 sn |
| Gemma 4 E2B-it QAT | 3.35 GB | 2.86 / 1.55 GB | v2 | %91 | %87 | %90 | 2.1 / 4.0 sn |
| **Gemma 4 E2B-it QAT** | | **2.85 / 1.54 GB** | **v3** | **%96** | **%93** | **%97** | **2.2 / 3.9 sn** |

Gemma v3 ayrıntı: JSON geçerliliği %100, anlamsal doğrulamayı geçme %99,5, tarih %97, saat %99, metrik değeri %100, "çıkarılacak bir şey yok" notları %100.

## Bellek düşürme denemesi (aynı model, Gemma 4 E2B)
Amaç: doğruluğu koruyarak belleği ~2 GB'a indirmek. Talimat v3; düşük bellek ayarları = bağlam 2048, `-fa on -ctk q8_0 -ctv q8_0 -ub 256 -b 512` (en uzun istem 1 522 token).

| Nicemleme | Ayar | Dosya | Çalışma / özel bellek | TR | EN | p50 / p95 |
|---|---|---|---|---|---|---|
| Q4_0 QAT | varsayılan | 3.35 GB | 2.85 / 1.54 GB | %96 | %93 | 2.2 / 3.9 sn |
| Q4_0 QAT | düşük bellek | 3.35 GB | 2.75 / 1.44 GB | %97 | %95 | 2.3 / 4.1 sn |
| **Q3_K_M** | **düşük bellek** | **2.54 GB** | **1.85 / 0.77 GB** | **%94 (%96\*)** | **%91 (%95\*)** | **2.2 / 4.9 sn** |

\* Rust'ta "gün adı var, `which` yok → `this`" varsayılanıyla yeniden puanlandığında. Q3_K_M'nin kaybettiği notların çoğu bu eksik alandan kaynaklanıyordu.

**Sonuç:** Q3_K_M + düşük bellek ayarları, doğruluğu koruyarak çalışma belleğini 1,85 GB'a (özel 0,77 GB) indiriyor; NFR-PERF-007'deki ≤ 2 GB bütçesine sığıyor. **Önerilen yapılandırma budur.**

## Ayrık test seti doğrulaması (TBD-10, 2026-10-01)
Talimat v3 dondurulduktan sonra yazılan, dev setle hiç ortak notu olmayan 100 TR + 100 EN'lik set (`testset.py`): farklı içerik, farklı referans günleri (Salı, Pazar, Cuma), gündelik/özensiz yazımlar ("yarin 8de", "gym tmrw"). Yapılandırma: Gemma 4 E2B Q3_K_M, talimat v3, düşük bellek ayarları, `which` yoksa `this` kuralı. Talimatta hiçbir değişiklik yapılmadı.

| | TR | EN |
|---|---|---|
| Alan doğruluğu | **%91** | **%91** |
| Kesinlik | %93 | %96 |
| Tam doğru not | %92 | %91 |
| Tarih / saat | %94 / %98 | %97 / %100 |
| Metrik değeri | %97 | %100 |
| Boş notlar | %100 | %100 |
| p50 / p95 | 2.1 / 4.7 sn | 2.0 / 4.4 sn |

Bellek: çalışma 1,85 GB, özel 0,77 GB. Dev sete göre ~5 puanlık düşüş, talimatın dev sete uyarlanmasından beklenen farkla uyumlu. **Sonuç SRS NFR-AIQ-001 hedefini (≥ %85) ve F1 kapısını (≥ %80) karşılıyor.**

**Kalan hata örüntüleri (17 not):**
1. Geçmiş gün adı metriklerde ("Cuma günü 80 kiloydum", "I was 80 kg on Friday") → şemaya `which: "last"` eklenmeli (2 not).
2. İngilizce olay/son tarih cümleleri ("registration opens on…", "leave starts on…", "results come out…") görev sayılmadı → talimatta "olay ve son tarihler de görevdir" (3 not).
3. Çoklu notlarda öğe atlama (kilo, uyku) (4 not).
4. Gün adı veya "haftaya/next" karışıklığı; "yarından sonra" → 1 (4 not).
5. `day_offset` ile `date`'in birlikte verilmesi → doğrulamada reddedildi (1 not); Rust'ta `day_offset` öncelikli sayılabilir.
6. İngilizce notlarda başlığın Türkçe yazılması (başlık anahtar sözcük oranı EN %49) → talimat düzeltmesi.

## Öğrenilenler
1. **Gramer kısıtı işe yarıyor:** Tüm modellerde sözdizimsel JSON geçerliliği %100.
2. **Şema sırası önemli (SRS C.3 düzeltmesi gerekli):** Gramer, özellikleri şemadaki sırayla ürettirir. `value` alanı `exercise/sets/reps`'ten önce olduğunda antrenman ağırlığı kayboluyordu. Sıra `kind, exercise, sets, reps, value, unit` yapılınca metrik doğruluğu %85 → %100.
3. **Talimat iyileştirmesi büyük fark yaratıyor:** Türkçe gün adı tablosu, "yalnız saat varsa bugün" kuralı ve iki ek örnek; küçük modellerde +12 puan.
4. **Rust kuralları:** Görevde yalnız saat varsa tarih = referans gün; gün adı var ama `which` yoksa `this`. SRS FR-EXT-012'ye eklenmeli.
5. **Kalan hatalar:** "ayın 5'i / the 5th" (ay çözümlemesi), geçmiş gün adı ("Pazartesi 79 kiloydum" → yanlış gün farkı), İngilizce notlarda başlığın Türkçe yazılması, ağırlıksız antrenmanda `value: 0`.
   - Öneri: şemaya `which: "last"` ve `day_of_month` alanları; başlık dili için talimat düzeltmesi; `value: 0` → boş kabul.

## Uyarılar (sonucu okurken)
- **İyimser tahmin:** Talimat v2/v3, bu setteki hatalara bakılarak düzeltildi. Yeni, daha önce görülmemiş 50–100 notluk bir **ayrık test seti** ile doğrulanmadan kesin sonuç sayılmamalı.
- **Donanım:** Ölçüm güçlü bir CPU'nun 4 iş parçacığıyla yapıldı. Gerçek 4 çekirdekli, minimum donanımda gecikme 1,5–2 kat olabilir (p95 ≈ 6–8 sn; hedef ≤ 10 sn).
- **Bellek:** Gemma'nın çalışma belleği 2,85 GB, NFR-PERF-007'deki "≤ 2 GB" sınırını aşıyor. Bunun ~1,3 GB'ı dosyadan eşlenmiş (bellek baskısında boşaltılabilen) model ağırlığı; özel bellek 1,54 GB. PRD v0.4'te bütçe "çalışma belleği ≤ 3 GB, özel bellek ≤ 2 GB" olarak güncellenmeli.
- Değerlendirme seti tek kişi tarafından yazıldı; gerçek kullanıcı notlarıyla genişletilmeli.

## Sonraki adımlar
1. Ayrık test seti (yeni 100 not) ile Gemma 4 E2B v3'ü doğrula; Qwen3-4B'yi yedek aday olarak tut.
2. SRS/PRD güncellemeleri: model seçimi (TBD-02), şema sırası (C.3), bellek bütçesi (NFR-PERF-007), yalnız-saat kuralı (FR-EXT-012).
3. Gerçek minimum donanımda (8 GB, 4 çekirdek) gecikme ölçümü.
4. F2 — Rust çekirdeği iskeleti.

## Dosyalar
`dataset.py` (set), `run_eval.py` (koşucu), `results/<model>.<sürüm>.jsonl` (örnek bazında), `results/summary.json` (özet). Modeller ve llama.cpp ikilileri git'e girmez (`bin/`, `models/`).
