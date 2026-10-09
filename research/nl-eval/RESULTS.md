# Doğal dil kalitesi — değerlendirme sonuçları (2026-10-10)

Model değişmedi: Gemma 4 E2B-it Q3_K_M, llama.cpp b11280, CPU 4 iş parçacığı. Değerlendirme, uygulamanın
kendi kodunu (asistan motoru `qa::engine::answer`; çıkarım → koruma → doğrulama) gerçek modelle çalıştırır:

```bash
cargo run --release -p pla-core --example nl_eval -- <etiket> [vaka-öneki]
```

Vakalar `crates/pla-core/tests/nl/` altında: 71 asistan + 31 çıkarım = 102 vaka; Ayberk'in ekran
görüntülerindeki hatalar ("selam", "130 kiloyum", "notlarıma ekle bunu", "sen karar ver", yapıştırılan antrenman
programı) ve varyasyonları, Türkçe ve İngilizce. Örnek bankası (`assets/qa_examples.jsonl`) vakalarla
birebir aynı cümle içeremez; düzenek bunu denetler.

## Sonuçlar

| Kategori | Başlangıç | v1 | v2 | v3 | v4 | Son |
| --- | --- | --- | --- | --- | --- | --- |
| asistan / dil | 8/14 | 12/14 | 14/14 | 14/14 | 14/14 | 14/14 |
| asistan / ölçüm | 5/13 | 11/13 | 13/13 | 13/13 | 13/13 | 13/13 |
| asistan / görev | 7/9 | 7/9 | 9/9 | 7/9 | 8/9 | 8/9 |
| asistan / görev tamamlama | 1/3 | 1/3 | 0/3 | 2/3 | 3/3 | 3/3 |
| asistan / not alma | 7/10 | 8/10 | 8/10 | 8/10 | 9/10 | 10/10 |
| asistan / not önerisi | 3/10 | 7/10 | 8/10 | 9/10 | 9/10 | 9/10 |
| asistan / sorgu | 8/9 | 9/9 | 8/9 | 9/9 | 9/9 | 9/9 |
| asistan / takip sorusu | 2/3 | 3/3 | 2/3 | 3/3 | 3/3 | 3/3 |
| çıkarım / antrenman programı | 6/9 | 9/9 | 9/9 | 9/9 | 9/9 | 9/9 |
| çıkarım / yapılan antrenman | 8/9 | 9/9 | 9/9 | 9/9 | 9/9 | 9/9 |
| çıkarım / kilo | 3/4 | 4/4 | 4/4 | 4/4 | 4/4 | 4/4 |
| çıkarım / diğer | 9/9 | 9/9 | 9/9 | 9/9 | 9/9 | 9/9 |
| **Toplam** | **67/102 (%66)** | 89 (%87) | 93 (%91) | 96 (%94) | 99 (%97) | **100/102 (%98)** |

v1 ve v2'de düzenekteki iki hata (kısa cevapta dil hakemi, Türkçe "İ" küçültmesi) birkaç vakayı yanlış
düşürüyordu; v2'den sonrası düzeltilmiş düzenekle.

## Ne değişti

- **Dil:** Türkçe kelime ve ekler ("kiloyum", "uyudum"), sonra önceki mesaj, sonra PLA'nın dili. Kural cevaba sızmıyor.
- **Dinamik örnekler:** karar adımı sabit 5 örnek yerine 85 örneklik bankadan en yakın 6'sını görür; mesaj istemin sonunda tekrarlanır.
- **Not:** `create_note` `notes/`a yazar, başlığı kendisi koyar, "bunu"yu önceki mesajdan alır; "X diye bir not" başlığı X olur; `suggest_note` yalnız Kaydet düğmesi önerir.
- **Görev tamamlama:** görevler kelime köküyle bulunur; yalnız bu soruda bulunan görev tamamlanabilir.
- **Kod yedeği (dar):** model yalnız cevap verdiğinde ama sözler açıksa (bulunan tek görev + "ödedim"; not isteğinden sonra "sen karar ver") araç kodla çağrılır.
- **Saat:** sabah/gece denmeyen 1–7 arası saat öğleden sonradır ("saat 3'te" → 15:00).
- **Çıkarım koruması:** tekrar aralığı ("4x4-6") veya plan sözcüğü olan bloktan antrenman çıkmaz; notta kg/lb yoksa antrenman ağırlığı kaydedilmez; hedef cümlesinden kilo çıkmaz. Bir nottaki egzersizler tek oturum sayılır.

## Kalan iki hata

- `suggest-toplanti`: "toplantıda bütçe 40 bin olarak onaylandı" — model cümleyi tekrar edip öneri sunmuyor (benzerleri geçiyor).
- `mixed-task-metric`: "dün 7 saat uyudum, yarın da 9'da toplantı var" — yalnız uyku kaydediliyor; ayrıca tarih dün yerine bugün.

## Hız ve bellek

- Soru başına ortanca süre: başlangıç 5,9 sn (çoğu soruda hiç araç çağırmıyordu), son 18 sn (araç çağrıları dahil). Aynı istem önbellekten 0,4 sn'de döner (`--cache-ram` ölçümü).
- llama-server'ın istem önbelleği 512 MiB ile sınırlandı (varsayılan 8 GiB; 100 soruda +1 GB büyümüştü). Bağlam boyutu (2048–8192) belleği yalnız ~20 MB değiştiriyor; bağlam ayrımı bu yüzden yapılmadı.
