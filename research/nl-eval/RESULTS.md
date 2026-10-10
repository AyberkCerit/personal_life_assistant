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

| Kategori | Başlangıç | v1 | v2 | v3 | v4 | İnceleme öncesi | **Son** |
| --- | --- | --- | --- | --- | --- | --- | --- |
| asistan / dil | 8/14 | 12/14 | 14/14 | 14/14 | 14/14 | 14/14 | **14/14** |
| asistan / ölçüm | 5/13 | 11/13 | 13/13 | 13/13 | 13/13 | 13/13 | **13/13** |
| asistan / görev | 7/9 | 7/9 | 9/9 | 7/9 | 8/9 | 8/9 | **8/9** |
| asistan / görev tamamlama | 1/3 | 1/3 | 0/3 | 2/3 | 3/3 | 3/3 | **3/3** |
| asistan / not alma | 7/10 | 8/10 | 8/10 | 8/10 | 9/10 | 10/10 | **10/10** |
| asistan / not önerisi | 3/10 | 7/10 | 8/10 | 9/10 | 9/10 | 9/10 | **9/10** |
| asistan / sorgu | 8/9 | 9/9 | 8/9 | 9/9 | 9/9 | 9/9 | **9/9** |
| asistan / takip sorusu | 2/3 | 3/3 | 2/3 | 3/3 | 3/3 | 3/3 | **3/3** |
| asistan / **olumsuz** (yazmamalı) | – | – | – | – | – | – | **5/5** |
| asistan / **saklı** (bankadan uzak) | – | – | – | – | – | – | **12/15** (+1 düzeltildi) |
| çıkarım (blok blok) | 26/31 | 31/31 | 31/31 | 31/31 | 31/31 | 31/31 | **31/31** |
| **Toplam** | **67/102 (%66)** | 89 (%87) | 93 (%91) | 96 (%94) | 99 (%97) | 100/102 (%98) | **117/122 (%96)** |

**Okurken dikkat:** son incelemenin uyarısıyla, eski vakaların çoğu örnek bankasındaki cümlelere çok yakın
(yalnız sayı ya da gün adı farklı); %98 genellemeden çok bilinen kalıpları ölçüyordu. Bu yüzden iki küme
eklendi: **saklı vakalar** bankaya bakmadan yazıldı ve düzenek, herhangi bir banka örneğine benzerliği (kelime
kökü Jaccard) 0,5'i aşanı reddeder; **olumsuz vakalar** inceleme bulgularından (görev tamamlanmamalı, not
açılmamalı). Yeni ifadelerdeki gerçekçi başarı saklı kümedeki **%80–87**'dir. Son koşudan sonra
`heldout-elektrikci` düzeltildi (model aynı aramayı tekrarlayınca yedek adım hiç çalışmıyordu) ve tek başına
yeniden çalıştırıldığında geçti; tam koşu yeniden yapılmadı.

v1 ve v2'de düzenekteki iki hata (kısa cevapta dil hakemi, Türkçe "İ" küçültmesi) birkaç vakayı yanlış
düşürüyordu; v2'den sonrası düzeltilmiş düzenekle. Çıkarım vakaları son turda uygulamanın yaptığı gibi
bloklara bölünerek çalışır (liste maddesi ayrı bir bloktur).

## Ne değişti

- **Dil:** Türkçe kelime ve ekler ("kiloyum", "uyudum"), sonra önceki mesaj, sonra PLA'nın dili. Kural cevaba sızmıyor.
- **Dinamik örnekler:** karar adımı sabit 5 örnek yerine 85 örneklik bankadan en yakın 6'sını görür; mesaj istemin sonunda tekrarlanır.
- **Not:** `create_note` `notes/`a yazar, başlığı kendisi koyar, "bunu"yu önceki mesajdan alır; "X diye bir not" başlığı X olur; `suggest_note` yalnız Kaydet düğmesi önerir.
- **Görev tamamlama:** görevler kelime ve fiil köküyle bulunur ("aradım" → "Annemi aramak"); yalnız bu soruda bulunan ve kullanıcının yaptığını söylediği görev tamamlanır: görevin fiili geçmiş zamanla ("ödedim" ~ "Fatura öde") ya da açık bir "tamamla/done". Soru kelimesi olan mesaj hiçbir şeyi tamamlamaz.
- **Kod yedeği (dar):** model yalnız cevap verdiğinde (ya da aynı çağrıyı tekrarladığında) ama sözler açıksa araç kodla çağrılır: yaptığını söylediği görev; not isteğinin hemen ardından "sen karar ver". Bu soruda başka bir yazma olduysa asla.
- **Kaydedilmiş not:** "bunu" ile işaret edilen mesaj zaten nota kaydedildiyse ikinci not açılmaz; öneri sunulunca model "kaydedildi" demez.
- **Saat:** yalnız dakikasız ve sabah/akşam denmeyen 1–7 arası saat öğleden sonradır ("saat 3'te" → 15:00); "7:30", "5:30am", "6'da kalk" olduğu gibi kalır.
- **Çıkarım koruması:** liste maddesi listesiyle birlikte okunur; tekrar aralığı ("4x4-6") veya plan kelimesi olan bir programdan antrenman da görev de çıkmaz; "plank", "split squat", "3x10 - 60 kg" yapılmış antrenmandır; notta kg/lb yoksa ağırlık kaydedilmez; kilo yalnız hedefin cümleciğindeyse düşer. Bir nottaki egzersizler tek oturum sayılır.

## Kalan hatalar

- `suggest-toplanti`, `heldout-gym-closed`: anlatılan bir olayda model öneri sunmayıp cümleyi tekrar ediyor.
- `mixed-task-metric`: "dün 7 saat uyudum, yarın da 9'da toplantı var" — yalnız uyku kaydediliyor (tarih de dün yerine bugün).
- `heldout-pedometre`: "pedometre 6400 diyor" — adım sayısı olarak tanınmıyor.

## Hız ve bellek

- Soru başına ortanca süre: başlangıç 5,9 sn (çoğu soruda hiç araç çağırmıyordu), son 18 sn (araç çağrıları dahil). Aynı istem önbellekten 0,4 sn'de döner (`--cache-ram` ölçümü).
- llama-server'ın istem önbelleği 512 MiB ile sınırlandı (varsayılan 8 GiB; 100 soruda +1 GB büyümüştü). Bağlam boyutu (2048–8192) belleği yalnız ~20 MB değiştiriyor; bağlam ayrımı bu yüzden yapılmadı.
