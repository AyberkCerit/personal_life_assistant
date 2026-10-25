# Asistan hafızası: okuma yolunun ölçümü (Aşama 1 kapısı)

Soru: hafıza dosyalarını (`memory/`) istemin içine koymak, notlarda olmayan ama hafızada olan soruları
cevaplatır mı ve notlardan bulunan bağlamı bozar mı?

## Nasıl çalıştırılır

```bash
$env:PLA_LLAMA_SERVER = "C:\dev\PLA\research\f1-model-eval\bin\llama-server.exe"
$env:PLA_EMBED_MODEL = "$env:LOCALAPPDATA\PLA\models\embeddinggemma-300M-Q8_0.gguf"
$env:PLA_MODEL = "C:\dev\PLA\research\f1-model-eval\models\gemma-4-E2B-it-Q3_K_M.gguf"
# arama (hızlı, ~1 dk)
cargo run --release -p pla-core --example retrieval_eval -- memory-p1
cargo run --release -p pla-core --example retrieval_eval -- memory-p1-nomem --no-memory
# asistan (model gerekir)
cargo run --release -p pla-core --example nl_eval -- <etiket> [id-öneki] [--no-memory]
```

`--no-memory` hafızayı dosyaya yazmaz ve yüklemez (boş `Recall`). `nl_eval` vakadaki `memory` alanını
geçici kasanın hafıza klasörüne `recall::render` ile yazar, `recall::load` ile yükler ve
`Question.recall`'a verir. `contains_any` ikinci bir "herhangi biri" grubudur; `contains` ile birlikte
verilirse ikisi de tutmalıdır.

## Dondurulan vakalar (commit `test(recall): freeze the memory eval cases`)

- `retrieval/vault.json`: 6 `none` vakası (IBAN, uçuş saati, tarif, vault'ta olmayan kişi, wifi şifresi,
  ehliyet sonucu; `facts: []`) ve 507 tokenlık bir `memory` fixture'ı (5 hakkımda, 3 kural, 3 durum,
  Deniz ve Efe için 3 kişi satırı). Fixture, mevcut 22 vakanın hiçbir olgusunu içermez.
- `nl/assistant.jsonl`: 8 geliştirme (`cat: memory`) ve 5 saklı (`cat: heldout`, id `memory-h-*`)
  vaka. Cevap yalnız hafızada: kişi olgusu (Deniz, Efe, Cenk), alışkanlık/saat (spor, laboratuvar günü,
  ulaşım, kahvaltı), alerji, telefon, kullanıcı kuralı (hitap, "Kolay gelsin") ve iki çıkarım durumu
  (`contains_any: görünüyor / notlarına göre / notlarında`). Uzunluk sınırı için düzenekte denetim
  olmadığından "kısa cevap" kuralı vakası yazılmadı. Saklı vakalar örnek bankasına Jaccard 0,5
  denetiminden geçti (düzenek başlangıçta denetler; koşu hata vermedi).

## Arama sonucu (EmbeddingGemma, `memory-p1c` ve `memory-p1c-nomem`)

Bütçe payları: son 48 saat + hafıza bloğu birlikte eski %40'ı oluşturur (`RECENT_TOKENS` + `MEMORY_TOKENS`,
her biri sabit); bulunan payı hafızadan önceki değerindedir, yalnız anılan kişilerin bloğu kadar küçülür.
Bu fixture'da hafıza bloğu 507 token.

| Mod | Hafızasız (Bağlı / Doğrudan) | Hafızalı (Bağlı / Doğrudan) | Ort. bulunan token (hafızasız → hafızalı) |
| --- | --- | --- | --- |
| kelime | 7/12 · 10/10 | 7/12 · 10/10 | 1143 → 1143 |
| kelime + bağ | 10/12 · 10/10 | 10/12 · 10/10 | 1328 → 1328 |
| hibrit | 10/12 · 10/10 | 10/12 · 10/10 | 1316 → 1316 |
| hibrit + bağ | 11/12 · 10/10 | 11/12 · 10/10 | 1472 → 1472 |

Hafızalı ve hafızasız koşu aynı: hibrit + bağ 21/22, hafıza bulunan notlara hiçbir şey kaybettirmiyor.
Doğrudan vakalarda gerileme yok. (`memory-p1.jsonl` ve `memory-p1-nomem.jsonl` dengelemeden önceki
sonuçlardır; güncel sonuç `memory-p1c*.jsonl`.)

### `none` vakaları (cevap vault'ta yok, 6 vaka)

| Mod | Ort. bulunan token | Ort. kaynak not |
| --- | --- | --- |
| kelime | 443 | 5,0 |
| kelime + bağ | 600 | 6,0 |
| hibrit | 1204 | 12,0 |
| hibrit + bağ | 1349 | 13,0 |

Hafızalı ve hafızasız aynı. Cevabı olmayan sorularda model hibrit modda yaklaşık 12 not ve 1200+ token okuyor.

### Geçmiş: dengelemenin nedeni

İlk uygulamada hafıza bloğu bulunan payın içinden harcanıyordu (`memory-p1`): hibrit + bağ **20/22**,
`linked-iha-ucuz` "6.800" olgusunu kaybetti, ort. bulunan token 1472 → 1341. Bu yüzden hafıza son 48
saat payının yanına, kendi diliminde alındı (karar L, düzeltme M); sonuç yukarıdaki tablo.

## Duman testi (gerçek model, tek vaka)

`memory-deniz-kahve`, Gemma 4 E2B Q3_K_M:

| Koşu | Sonuç | Cevap |
| --- | --- | --- |
| hafızalı | PASS (21,8 sn) | "Deniz kahveyi sütsüz ve şekersiz içer." |
| `--no-memory` | FAIL (19,9 sn): cevapta "sütsüz" yok | |

Düzenek hafızayı yazıyor, yüklüyor ve cevap yalnız hafızayla çıkıyor.

## Model koşuları

Gemma 4 E2B Q3_K_M. Hafıza vakalarında istem dört turda ayarlandı (`memory-p1`…`p4`, 2026-10-19; dosyalar
orijinal worktree'de kaldı, commit edilmedi):

| Tur | Hafızasız | Hafızalı |
| --- | --- | --- |
| p1 | 0/13 | 8/13 |
| p2 | 0/13 | 9/13 |
| p3 | 0/13 | 10/13 |
| p4 | 0/13 | 9/13 |

### Kapı koşusu (`memory-p5-all`, `memory-p5-all-nomem`, 2026-10-23)

Aynı kod (PR #10 dalı, main ile birleştirilmiş, `7bb5f98`), 152 vakanın tamamı:

| | Hafızasız | Hafızalı |
| --- | --- | --- |
| Hafıza vakaları (13) | 0/13 | 9/13 |
| Eski vakalar (138) | 122/138 | 121/138 |
| Toplam | 123/152 (40 dk) | 131/152 (64 dk) |

- Hafıza vakaları hafızasız hiç geçmiyor, hafızalı 9/13 geçiyor.
- Eski vakalarda iki koşu arasındaki tek fark `lang-thanks` (hafızalıda İngilizce soruya Türkçe cevap).
  Hafızalı koşuda yeni geçen eski vaka yok.
- Kalan 4 hafıza hatası: iki çıkarım vakası (`memory-bitirme-govde`, `memory-h-kabuk`) çekincesiz
  cevaplanıyor; iki saklı vaka (`memory-h-cenk-oyun`, `memory-h-telefon`) hafızada olduğu halde "notlarda
  bulamadım" diyor.
- Hafızalı koşu 1,6 kat uzun sürdü (hafıza bloğu her istemde ~500 token). Gerçek uygulamada istem
  önbelleği bunu büyük ölçüde karşılamalı, `qa_probe` ile ölçülmedi.

**Kapı: geçti.** Hafızanın kendisi eski vakalarda gerileme yapmıyor (fark 1 vaka, dil vakası).

### Ayrı bulgu: main'deki dil gerilemesi

10-19 koşusunda (`memory-p3-all`, PR #4 öncesi kod) geçen 11 İngilizce vaka bu koşuda **iki modda da**
kalıyor: İngilizce soruya Türkçe ("Senin…", "Sen…") cevap (`metric-*-en`, `task-rent-en`, `note-save-en`,
`query-*-en`, `heldout-scale-en`, `heldout-sleep-avg`, `lang-thanks`). Neden PR #4'ün (asistan adı/hitap)
sistem istemine her zaman eklediği "In Turkish, speak to the user as sen" cümlesi: varsayılan (sen)
ayarda bile eklendiği için küçük model bunu Türkçe cevap işareti olarak okuyor. Hafızayla ilgisi yok,
main'de de aynı kod var.

- [x] `qa_probe -- <ref>` hafızalı ve hafızasız: ilk cevap süresi ve `cache_n` (aşağıda)

## Sohbetten hafızaya yazma (Aşama 2, Task 8)

Soru: sohbet adımı (`recall::extract::request` → model → `parse` → `guard`) yalnız kullanıcının kalıcı
olarak söylediğini mi yazıyor?

```bash
cargo run --release -p pla-core --example nl_eval -- <etiket> memory          # 42 vaka
cargo run --release -p pla-core --example nl_eval -- <etiket> chat-mem-d      # yalnız geliştirme
```

`assistant.jsonl`'deki Task 5 vakaları da `memory-` ile başladığı için bu vakaların kimliği `chat-mem-`,
dosya adı `memory` ile seçilir (plandaki `memory-` öneki ikisini birden koşardı).

### Dondurulan vakalar (commit `test(recall): freeze the memory extraction cases`)

`tests/nl/memory.jsonl`, istem ayarlanmadan önce: 30 geliştirme, 12 saklı. Kendisi (alışkanlık, sağlık,
iş, kural, proje durumu), 10 kişi vakası (ikisi aynı mesajda iki kişi, biri akrabalık eki "Annemin",
ikisi İngilizce), yazılmaması gereken 14 vaka (olumsuz, varsayım, soru, dilek, koşul, plan, yalnız
bugüne ait, asistanın önerisine "tamam"), konu güncelleme, iki "unut", reddedilen satırın yanında yeni
bilgi, "bunu hatırla" ile önceki mesaj. Saklı vakalar geliştirme vakalarına ve istemdeki örneklere
Jaccard 0,5 denetiminden geçer (düzenek her koşuda başta denetler).

- **Yanlış yazım:** guard'dan geçip beklenen hiçbir kayda uymayan her `Add` ya da `Forget`.
- **Doğru yazım:** beklenen kayıtlardan bulunanlar.

### Ayar turları (yalnız geliştirme kümesi, 2026-10-24)

| Tur | Vaka | Doğru yazım | Yanlış yazım | Değişiklik |
| --- | --- | --- | --- | --- |
| v1 | 17/30 | 8/21 | 2 | Task 7 istemi, örneksiz. Model her kaydı `people` dosyasına, kişiyi "User" diye yazdı; konu genel ("facts-and-preferences"). |
| v2 | 16/30 | 11/21 | 12 | 7 örnek (few-shot), alan sırası `evidence, file, person, topic, text`, `max_tokens` 600 (v1'de iki cevap kesildi). Artık fazla yazıyor. |
| v3 | 22/30 | 14/21 | 4 | Guard: bugüne/haftaya ait ve gelecek zamanlı cümle, unutma isteği, `person` doluysa Kişiler. Akrabalık sözcüğü ekli eşleşir. Kullanıcı turunda "Write text in …". |
| v4 | 24/30 | 15/21 | 1 | Zaman kontrolü yan cümle düzeyinde; "akşam" `-sam` koşul eki sayılmıyordu (d28'i v2'den beri düşüren yanlış pozitif). Alıntı kök eşleşmesiyle de kabul (bütün mesaj okunur). Dil uyuşmazlığı, "yapalım"/"let", unut isteğinde önceki mesaj. |
| v5 | 28/30 | 19/21 | 0 | Yanlış dildeki satır yerine kullanıcının kendi sözü; model kişiyi boş bırakınca akrabalık sözcüğünden ad ("Annemin" → "Annem"); rutin örneği. |

### Son koşu (`memory-v5`, saklı küme ilk ve tek kez)

| Küme | Vaka | Doğru yazım | Yanlış yazım |
| --- | --- | --- | --- |
| Geliştirme | 28/30 | 19/21 (%90) | 0 |
| Saklı | 11/12 | 8/9 (%89) | **0** |

**Kapı: geçti** (saklıda yanlış yazım 0, geliştirmede doğru yazım ≥ %80).

Kalanlar, üçü de yazmama yönünde:
- `chat-mem-d27`: "Bunu unut" (önceki mesaj) konuyu unutmuyor; model `forget` yerine iki kayıt yazıyor, guard ikisini de atıyor.
- `chat-mem-d28`: model iki bilgiyi tek kayıtta birleştirip bütün cümleyi kanıt veriyor; "Bugün" geçtiği için atılıyor.
- `chat-mem-h03` ("Kod örneklerini Rust ile yaz lütfen."): model boş döndü.

Notlar:
- İngilizce mesajlara model örneklere rağmen Türkçe metin yazıyor; guard o zaman kullanıcının sözünü olduğu gibi yazar ("I work night shifts at the hospital.").
- Reddedilene Jaccard ≥ 0,5 olan öneri, kullanıcı aynı şeyi açıkça yeniden söylese de atılıyor (Task 7 testi). Spec "kullanıcı yeniden söylerse yazılır" diyor; bu vakalarda o durum yok, karar açık.
- Asistanın sohbet ve çıkarım istemleri değişmedi; eski 138 nl-eval vakası bu kodu kullanmaz, yeniden koşulmadı.

## Kalan hafıza hataları (`memory-p6-all`, 2026-10-24)

### Nedenler

- **Çıkarım çekincesiz** (`memory-bitirme-govde`, `memory-h-kabuk`): hafıza bloğunda satır
  `- According to your notes ([[Not]], tarih): Bitirme projesinde Skywalker X8 öne çıkıyor.` biçimindeydi.
  Model Türkçe yazarken İngilizce etiketi atıp iki noktadan sonraki cümleyi kelimesi kelimesine
  kopyalıyordu; kural 8 bunu yasaklasa da uymadı.
- **"Notlarda bulamadım"** (`memory-h-cenk-oyun`, `memory-h-telefon`): ikisinde de karar adımı
  `search_notes` çağırmıştı (örnek bankasındaki "Ali'nin telefon numarası neydi → search_notes" gibi
  örnekler). Yazma isteminde modelin en son okuduğu şey `Tool result: {"notes":[]}` oluyor, kişi/hafıza
  bölümünde cevap dururken "bulamadım" diyordu. Geliştirme benzeri `Ali'nin en sevdiği yemek hangisi?`
  (hafızada "mantı") aynı istemle iki koşuda da aynı hatayı verdi; istem dökümünde hafıza satırı ve
  "Answer from the memory above" cümlesi yerindeydi.

### Değişiklikler

1. `recall/load.rs`: çıkarım satırı çekinceyi iki noktadan sonra, satırın kendi dilinde tekrarlar:
   `… (…): Notlarına göre, Bitirme projesinde …`. Kural 8'in metni değişmedi (satır hâlâ
   "According to your notes" ile başlıyor).
2. `qa/engine.rs` `with_hedge`: cevap bir çıkarım satırını kullanıyorsa (notunu anıyor ya da uzun
   kelimelerinin yarısını taşıyor) ve hiç çekince ("göre", "notlarında", "görünüyor", "according to"…)
   yoksa başına "Notlarına göre, " / "According to your notes, " eklenir. Panel `Done`'da kayıtlı
   metni gösterdiği için önek kullanıcıya ulaşır (akış sırasında görünmez).
3. `qa/engine.rs` `messages`: istemde hafıza ya da kişi bölümü varken `search_notes` sonucunun yanına
   "If these notes do not answer the question, the memory sections above may: answer from them. Memory
   is not a note: give it no [[link]]." eklenir. Son cümle, ilk denemede görülen "[[Note title]]"
   yer tutucusu kopyalamasını kaldırdı.

Hafıza yokken istem ve cevap değişmez (birim testleri: `a_search_result_points_back_to_the_memory`,
`an_inferred_memory_line_is_never_said_as_a_fact`, `without_memory_the_rules_are_the_original_ones`).

**Denenip geri alınan:** yazma talimatının sonuna hafıza hatırlatması ("If the memory section answers my
message…; begin your answer with "Notlarına göre"") çekinceyi hiç sağlamadı, `memory-alerji`'yi de
düşürdü.

### Yöntem: saklı vakalar

Ayar yalnız 8 geliştirme vakası ve commit edilmeyen 6 geçici benzerle yapıldı (çıkarım: "tez konum ne
olacak gibi duruyor?"; arama yolu: Ece/film, Ali/yemek; ayrıca bilgisayar, kulaklık; bir saat sorusu
belirsiz olduğu için sayılmadı). Her değişiklik ilgili vakalarda iki koşuyla denendi. Saklı `memory-h-*`
vakaları yalnız sondaki tam koşuda ölçüldü; onlardan bilinen tek şey p5'teki cevap metinleriydi (görev
tanımında verilen hata türü).

### Sonuç

Gemma 4 E2B Q3_K_M, 152 vaka, hafızalı, kod `0e25ee0` (çift çekince düzeltmesinden önce), 59 dk:

| | p5 (`7bb5f98`) | p6 |
| --- | --- | --- |
| Hafıza vakaları (13) | 9/13 | **12/13** |
| Eski vakalar (138) | 121/138 | 130/138 |
| Toplam | 131/152 | **143/152** |

- Dört hedef hatanın dördü geçti: `memory-bitirme-govde`, `memory-h-kabuk` ("Notlarına göre, …"),
  `memory-h-cenk-oyun` ("Hades"), `memory-h-telefon` ("Pixel 8").
- p6'da `memory-bitirme-govde` "Notlarına göre, Notlara göre, …" yazdı: model kendi çekincesini
  "Notlara göre" diye kurmuş, koruma bu biçimi tanımıyordu. Her "göre" / "according to" artık çekince
  sayılıyor; vaka bu düzeltmeyle iki koşuda da tek önekle geçti. Tam koşu bu düzeltmeden önce yapıldı.
- `memory-alerji` kaldı ("özel bir not bulunmuyor"). İstemi değişmedi (çıkarım satırı ve arama yok);
  `5abf257` koduyla da 1/2, bu kodla 0/3. Hafızayı okuma değil, "fıstık alerjisi → yemek" çıkarımı
  sorunu; kararsız.
- Eski vakalardaki artış (+9) çoğunlukla main'e giren dil düzeltmesinden (İngilizce vakalar,
  `lang-thanks`); hafızasız istemde değişiklik yok. İki eski vaka kaldı: `note-bunu-not-al`
  (İngilizce "Not alındı" algısı) ve `heldout-elektrikci` (`complete_task` yok). İkisi de `5abf257`
  koduyla iki koşuda aynı biçimde kalıyor: bu değişikliğin gerilemesi değil.
- Hafızasız tam koşu tekrarlanmadı: hafıza yokken istem bayt bayt aynı (birim testleri).
- Koşu `5abf257` üstünde yapıldı. Dal sonra `b05cc55`'e taşındı (persona düzeltmesi `3236c6d`
  `voice`/`write_instruction`'ı değiştirdi); bu taban üstünde tam koşu tekrarlanmadı.

## Süre ve önbellek (`qa_probe`, 2026-10-24)

Referans veri seti `make_reference -- %TEMP%\pla-reference 10000 0` (10 000 not, eksiz), `perf` ile
indekslendi. `qa_probe --memory`, `tests/retrieval/vault.json`'daki hafıza fixture'ını (14 satır)
geçici bir kasadan yükler; referans kasaya yazmaz. Makinede aynı anda başka oturumların koşuları
olabilirdi, mutlak süreler kaba.

| | Hafızasız | Hafızalı |
| --- | --- | --- |
| Önceden okuma: istem / süre | 705 tok / 6,6 s | 1346 tok / 21,0 s |
| Soru 1 karar: yeni / önbellek (`cache_n`) / süre | 742 / 700 / 14,4 s | 742 / 1341 / 15,3 s |
| Soru 1 toplam (araçlı, `query_tasks`) | 51,6 s | 39,1 s* |
| Soru 2 karar: yeni / `cache_n` / süre | 701 / 700 / 12,7 s | 701 / 1341 / 13,9 s |
| Soru 2 toplam (araçsız) | 14,9 s | 15,3 s |

\* Fark cevabın uzunluğundan (338 / 113 token üretildi), hafızadan değil.

- Hafıza bloğu (~640 token) önceden okumada bir kez okunuyor ve her soruda tamamen önbellekten geliyor:
  sorunun yeni token sayısı hafızalı ve hafızasız aynı. Soru başına maliyet ~1 s (daha uzun bağlam).
- `nl_eval`'deki 1,6 katlık süre farkı düzenekten: her vaka yeni bir hafıza ile soğuk başlıyor, önceden
  okuma yok.
- Önceden okuma (panel açılınca) hafızayla 6,6 → 21 s uzadı.

## Notlardan Durum çıkarımı (Aşama 3, Task 10, 2026-10-24)

Soru: bakım penceresindeki not adımı (`recall::infer::request` → model → `changes`) yalnız notun söylediği
güncel iş durumunu mu yazıyor? Not vakaları `tests/nl/memory.jsonl` içinde, kimlikleri `note-mem-`.

```bash
cargo run --release -p pla-core --example nl_eval -- <etiket> note-mem      # 40 not vakası
cargo run --release -p pla-core --example nl_eval -- <etiket> note-mem-d    # yalnız geliştirme
```

- **Yanlış yazım:** guard'dan geçip beklenen hiçbir satıra uymayan her `Add`, ve vakanın adlandırmadığı
  her düşürme (`Forget`, "source changed"). `optional` işaretli satır yanlış sayılmaz, beklenen de sayılmaz.
- **Doğru yazım:** beklenen satırlardan ve düşürmelerden bulunanlar.

### Vakalar ve saklı kümelerin sırası

1. `e9a51cd` (`test(recall): freeze the note inference cases`): 20 geliştirme + 10 saklı (h01–h10), istem ve
   ölçümden önce. Yazılması gerekenler (proje, karar, başvuru, yapılacaklar listesi, İngilizce), yazılmaması
   gerekenler (tarif, kitap notu, nasıl-yapılır, varsayım/soru, koşul, başkalarının haberi, günlük), güncelleme,
   notun artık vermediği satırı düşürme, reddedilen satır.
2. Ayar yalnız geliştirme kümesinde (aşağıdaki tablo, v1–v3).
3. Eski saklı küme ilk ve tek kez v3 ile koşuldu (`note-v3-heldout`): **7/10, doğru yazım 3/6, yanlış yazım 3,
   kapı geçmedi.** h04: model eski satırın konusunu değiştirdi (`dil-kursu` → `almanca`); eski satır düştü,
   yenisi başka konuda yazıldı (bir yanlış yazım + bir yanlış düşürme). h05: model notta artık olmayan eski
   bilgiyi ("Bisiklet tamirde") yeni satıra taşıdı. h06 yalnız eksik yazım (reddedilenle birleşik satır atıldı).
4. h01–h10'a dokunulmadı. Sonuç görüldüğü için bu vakaların `cat`'i `seen` yapıldı; artık kapıya girmezler.
5. `e64b516` (`test(recall): freeze a second held-out note set`): yeni saklı küme h11–h20, h04/h05'in
   gösterdiği iki genel düzeltmeden **önce** donduruldu ve commit'lendi.
6. Düzeltmeler (v4): tek eski satır kaldıysa ve model tek bir satırı yeni konuyla verdiyse, satır eski konuyla
   güncellenir. Eski satırın notta artık geçmeyen bir sözcüğünü taşıyan satır yazılmaz.
7. `note-v4`: yeni saklı küme ilk kez koşuldu. **Kapı sonucu bu koşudur.**

### Ayar turları (yalnız geliştirme kümesi)

| Tur | Vaka | Doğru yazım | Yanlış yazım | Değişiklik |
| --- | --- | --- | --- | --- |
| v1 | 13/20 | 8/11 | 7 | İstem + 4 örnek. İngilizce notlara Türkçe satır. "bütçe" konusu eski "butce"yi düşürdü. Reddedilen bilgi birleşik satırda geri geldi. Tarif, nasıl-yapılır ve günlük notları yazıldı. |
| v2 | 17/20 | 11/11 | 3 | Notun dilinde olmayan satır yerine notun kendi sözü. Eski konuyla ASCII katlanmış eşleşme. Kanıt da reddedilenle karşılaştırılır. Günlük, Excel ve bisiklet nasıl-yapılır örnekleri (7 örnek). |
| v3 | 20/20 | 11/11 | 0 | Başlık satırı kanıt sayılmaz. İkiden çok cümlelik kanıt atılır (bütün bir tarif tek durum değil). Genel geniş zaman edilgen ("sulanır", "düzenlenir") `qa::lang::is_general` ile atılır. |

### Kapı koşusu (`note-v4`, 2026-10-24)

| Küme | Vaka | Doğru yazım | Yanlış yazım |
| --- | --- | --- | --- |
| Geliştirme | 20/20 | 11/11 | 0 |
| Görülen (h01–h10, artık saklı değil) | 9/10 | 5/6 | 0 |
| **Saklı (h11–h20)** | 6/10 | 3/6 | **5** |

**Kapı: geçmedi** (saklıda yanlış yazım 0 hedefi). Beş yanlış yazımdan:
- İkisi guard hatası, ikisi de yazmama yönünde ve birer yanlış düşürme sayıldı. h12: "için" gibi kısa
  sözcükler "eski bilgi" sayıldı ve doğru güncelleme atıldı. h16: kanıt başlık satırıyla başlıyordu.
  Sonradan düzeltildi: kökler ≥ 5 harfli sözcüklerden alınıyor, kanıtın başındaki başlık atlanıyor.
- Üçü gerçek yanlış yazım, notta olmayan bir durumu Durum'a koyar:
  - h15: fiilsiz liste notu ("Passport, charger, two shirts, sunscreen.").
  - h17: başkalarının haberi, duyulan geçmiş ("taşınacakmış", "artırmış").
  - h20: günlük notunda o güne ait olay ("kütüphanede çalıştım, makarna yaptım").

`note-v5`, yalnız bilgi için (iki guard hatası düzeltilmiş, saklı küme artık görülmüş): geliştirme 20/20 (0
yanlış), görülen 9/10 (0), saklı 7/10, doğru yazım 5/5, **yanlış yazım 3** (h15, h17, h20).

Sonraki adım için öneriler (yapılmadı):
- Guard'lar: fiilsiz liste satırı; `-mış/-miş` duyulan geçmişle biten cümle; `daily/` notlarında
  yalnız geçmiş zamanlı birinci tekil olay.
- Ya da ilk sürümde günlük notları çıkarımdan çıkarmak.
- Her iki yolda da üçüncü bir saklı küme, düzeltmelerden önce dondurulmalı.

### Süre (not başına, `note-v4`, 40 vaka, CPU)

| | s |
| --- | --- |
| İlk istek (soğuk, örnekler önbelleğe giriyor) | 30,5 |
| Sonrakiler, ortalama | 4,6 |
| Medyan | 4,9 |
| En uzun (ilk hariç) | 9,3 |

Pencere başına en fazla 10 not: soğuk başlangıçla ~75 s, sıcak ~45 s model süresi. İstem ~1,4K token
örnek içeriyor; `cache_prompt` ile tekrarlanan baş kısım yeniden okunmuyor.

### Üçüncü tur: liste, duyulan geçmiş ve günlük notu guard'ları (2026-10-24)

Sıra:
1. `0600592` (`test(recall): freeze a third held-out note set`): h11–h20 `note-v4`'te bir kez saklı olarak
   koşulduğu için `seen` yapıldı. Yeni saklı küme h21–h30, guard'lardan önce donduruldu. Jaccard 0,5 denetimi
   koşunun başında geçti.
2. Guard'lar (`qa::lang`):
   - `is_list`: virgülle ayrılmış 3+ kısa öğe ve hiçbirinde fiil eki yok.
   - `is_hearsay`: bir yan cümle `-mış/-miş/-mişler` ile bitiyor ("geçmiş" hariç).
   - `is_day_event`: tarih adlı notlarda (`2026-10-23.md`) gün parçası (sabah, öğle(den), akşam, gece,
     morning, afternoon, evening, tonight) geçen cümle atılır.
3. `bfeebf3`: geliştirme kümesine d21 eklendi. Günlük notta gerçek ilerleme ile akşam olayı yan yana; guard'ın
   ilerlemeyi düşürmediğini geliştirme vakasıyla da göstermek için.

**Sızıntı notu:** h21–h30'u da guard'ları da aynı oturum yazdı. Günlük notu guard'ı, plandaki "geçmiş zamanlı
birinci tekil olay" tanımı yerine gün parçası kuralıyla tasarlandı. Bu karar verilirken h21 ve h30 okunmuştu:
birinci tekil kural bu iki vakadaki gerçek ilerlemeyi ("taslağı gönderdim", "raporu teslim ettim") de
düşürürdü. Kuralın geliştirme/görülen dayanağı d14, h20 (düşürülmesi gereken akşam/öğleden sonra olayı) ve
sonradan eklenen d21 (kalması gereken ilerleme). h21 ve h30 için kapı sonucu bağımsız değildir; aşağıda
ayrı verildi.

| Küme | Koşu | Vaka | Doğru yazım | Yanlış yazım |
| --- | --- | --- | --- | --- |
| Geliştirme (d01–d20) | `note-v6` | 20/20 | 11/11 | 0 |
| Geliştirme d21 | `note-v6-note-mem-d21` | 1/1 | 1/1 | 0 |
| Görülen (h01–h20) | `note-v6-note-mem-h0`, `-h1`, `-h20` | 19/20 | 10/11 | 0 |
| **Saklı h21–h30** | `note-v6-heldout-*`, ilk ve tek kez | 9/10 | 5/5 | **1** |
| Saklı, h21 ve h30 hariç | aynı koşu | 7/8 | 3/3 | **1** |

**Kapı: geçmedi** (saklıda yanlış yazım 1). Kalan hata:
- h27 (İngilizce): "Apparently Sam is leaving the company. Priya got promoted." olduğu gibi yazıldı.
  Başkalarının haberi. `is_hearsay` yalnız Türkçe `-mış` ekini tanıyor; İngilizcede duyulan haber işareti
  ("apparently", "I heard") ve kullanıcı dışında birinin özne olması denetlenmiyor.

Geçen kalıplar: liste (h22, h29), Türkçe duyulan geçmiş (h23), günlük olay (h25 ve h21'in akşam cümlesi).
Fiilli yapılacaklar listesi (h24) ve günlük nottaki gerçek ilerleme (h21, h30) yazıldı.

Not başına süre bu turda da aynı: ilk istek soğuk ~20 s, sonrakiler 1,3–6,1 s.

Sonraki adım için öneri (yapılmadı): İngilizce duyulan haber işaretleri ("apparently", "I heard", "reportedly")
ve özne kullanıcı değilken (üçüncü tekil ad) satırı yazmama. Ardından dördüncü bir saklı küme gerekir, yine
düzeltmelerden önce dondurulmuş olarak.

### Dördüncü tur: İngilizce duyulan haber (2026-10-24)

Sıra:
1. `a035615` (`test(recall): freeze a fourth held-out note set`): h21–h30 `note-v6-heldout`'ta bir kez saklı
   olarak koşulduğu için `seen` yapıldı. Yeni saklı küme h31–h40 guard'dan önce donduruldu. İçerik: İngilizce
   ve Türkçe duyulan haber, başka biri geçse de kullanıcının kendi durumu, liste, karışık günlük not. Mevcut
   guard'ların geçireceği vakalar seçilmedi. Jaccard 0,5 denetimi model koşmadan geçti. Bu küme yazıldıktan sonra
   kapı koşusuna kadar guard tasarımı için tekrar okunmadı.
2. `a0e76db`: geliştirme kümesine d22–d25 eklendi: İngilizce duyulan haber işaretleri, başkalarının haberi,
   başka biri geçen kendi durum ("I'm the best man at Tom's wedding…") ve kişi adıyla değil bir eşyayla
   açılan durum satırı ("Bike rack is installed.").
3. Guard (`qa::lang`):
   - `is_hearsay` artık İngilizce işaretleri de tanıyor: "apparently", "reportedly", "supposedly",
     "allegedly", "rumo(u)r", "word is", "they say", "heard that", "I heard" ("I heard back" hariç).
   - Yeni `is_person_news`: İngilizce cümle büyük harfli bir adla ve "is/got/has/left/moved…" fiiliyle
     açılıyor, kullanıcı (I/my/we/our…) cümlede yok. Notun başlığındaki sözcükle açılan cümle haber
     sayılmaz ("Website is live" bir "Website" notunda). Ad, katlanmamış kanıtta aranır.

| Küme | Koşu | Vaka | Doğru yazım | Yanlış yazım |
| --- | --- | --- | --- | --- |
| Geliştirme d01–d25 | `note-v7-note-mem-d` | 25/25 | 14/14 | 0 |
| Görülen h01–h30 | `note-v7-note-mem-h0`, `-h1`, `-h2`, `-h30` | 29/30 | 15/16 | 0 |
| **Saklı h31–h40** | `note-v7-heldout-*`, ilk ve tek kez | 8/10 | 3/5 (%60) | **0** |

**Kapı: geçti** (saklıda yanlış yazım 0).

Eksik kalan iki doğru yazım, ikisi de yazmama yönünde. Model iki cümleyi tek kanıtta birleştirdi; birinin
guard'a takılması bütün satırı düşürdü:
- h36: "Sabah dişçiye gittim. Bitirme tezinin sunumu 9 Aralık'a kesinleşti." Gün parçası "sabah" yüzünden
  tez sunumu da yazılmadı.
- h40: "Word is the chess club will move to Thursdays. I'm still preparing for the regional tournament…"
  Duyulan haber işareti yüzünden turnuva hazırlığı da yazılmadı.

Olası iyileştirme (yapılmadı): kanıtı cümlelere bölüp guard'a takılmayan cümleyle satırı kurmak. Bu,
doğru yazımı artırır ama satır metni modelin birleşik cümlesinden gelir; ayrı ölçüm ister.

Not başına süre bu koşuda daha yüksek: soğuk ilk istek 36–39 s, sonrakiler 2,7–11,6 s. Nedeni ölçülmedi
(makinede başka yük olabilir); önceki turlardaki 4–5 s sıcak ortalama karşılaştırma için durur.
