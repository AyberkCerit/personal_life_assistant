# F1 — Model Fizibilite Denemesi Sonuçları (2026-09-30)

**Karar: GEÇTİ.** Gemma 4 E2B-it (Q4_0 QAT), talimat v2 ve şema sırası düzeltmesiyle (v3), karar kapısını (alan doğruluğu ≥ %80, not başına ≤ 10 sn) iki dilde de aştı. Kendi model eğitimine (yedek plan) şimdilik gerek yok.

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

## Öğrenilenler
1. **Gramer kısıtı işe yarıyor:** Tüm modellerde sözdizimsel JSON geçerliliği %100.
2. **Şema sırası önemli (SRS C.3 düzeltmesi gerekli):** Gramer, özellikleri şemadaki sırayla ürettirir. `value` alanı `exercise/sets/reps`'ten önce olduğunda antrenman ağırlığı kayboluyordu. Sıra `kind, exercise, sets, reps, value, unit` yapılınca metrik doğruluğu %85 → %100.
3. **Talimat iyileştirmesi büyük fark yaratıyor:** Türkçe gün adı tablosu, "yalnız saat varsa bugün" kuralı ve iki ek örnek; küçük modellerde +12 puan.
4. **Rust kuralı:** Görevde yalnız saat varsa tarih = referans gün. SRS FR-EXT-012'ye eklenmeli.
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
