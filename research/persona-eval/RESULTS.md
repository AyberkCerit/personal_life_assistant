# Persona uyumu: ad, hitap, sen/siz (gerçek model)

Tarih: 2026-10-24. Model: Gemma 4 E2B Q3_K_M (llama.cpp), `nl_eval` üstünden uygulamanın kendi
`qa::engine::answer` kodu. Vakalar: `crates/pla-core/tests/nl/assistant.jsonl`, `persona-*` (25).
Ham çıktılar: `research/nl-eval/results/persona-*.jsonl`.

## Ne ölçülüyor

| grup | vaka | denetim |
| --- | --- | --- |
| `persona-siz-*` | 9 | siz + "Ayberk Bey": cevapta "sen" biçimi yok (`expect.formal`), hitap geçiyor, içerik doğru |
| `persona-hitap-*` | 2 | sen + "Ayberk": hitap geçiyor |
| `persona-ad-*` | 4 | ad "Atlas": "adın ne / sen kimsin / what's your name" cevabında Atlas geçiyor (biri siz ile) |
| `persona-en-*` | 10 | siz ayarlıyken İngilizce soru İngilizce cevaplanıyor, içerik doğru; `en-today-*` aynı soruyu ayarları ayrı ayrı açarak soruyor, `-default` persona'sız kontrol |

`expect.formal`'ın "sen" denetimi kaba: zamir (sen, sana, senin…), -sın/-sin/-sun/-sün, -rsan/-rsen
ve çoğul iyelik (-ların, -lerinde…). "kesin", "olsun", "kolay gelsin" gibi sözcükler hariç
tutuluyor. Hiçbir koşuda bu denetim kalmadı. Siz cevaplarının hepsi elle de okundu ve gözden
kaçan bir "sen" biçimi bulunmadı.

## Sonuçlar

| koşu | istem | sonuç | hatalar |
| --- | --- | --- | --- |
| persona-v1 | main (5abf257) | 20/22 | `siz-yarin`, `hitap-yarin`: tek cümlelik görev cevabında hitap hiç geçmiyor |
| persona-v2 | + son talimat "call me X once" | 23/23 | (`en-today`'in içerik denetimi yoktu; cevap "bulamadım" diyordu) |
| persona-today-v2 | aynı | 2/4 | `en-today`, `en-today-formal`: siz ayarlıyken İngilizce soruda notu kullanmıyor |
| persona-v3 | + siz cümlesi sistem isteminden çıkarıldı | 24/25 | `en-today-address` (gürültü, aşağıda) |
| persona-today-v3 ×3 | aynı | 9/12 | `en-today-address` 2, `en-today-default` 1 |

`en-today` vakaları, en son istemle 4 koşu boyunca:

| ayar | geçen |
| --- | --- |
| siz + hitap | 4/4 |
| yalnız siz | 4/4 (düzeltmeden önce 0/1, siz + hitap 0/3) |
| yalnız hitap | 1/4 |
| persona yok | 4/5 |

Siz + hitap ile yalnız hitap düzeltmeden sonra İngilizcede **aynı istemi** alıyor ama sonuçları
farklı. Persona'sız kontrol de bir kez kalıyor. İngilizce "what did I do today?" sorusunda
Türkçe günlük notunu kaçırmak modelin persona'dan bağımsız, sınırda kalan bir davranışı.
Sunucunun önceki vakadan kalan önbellek durumu sıradaki cevabı etkiliyor gibi görünüyor.

Gerileme (en son istem): `lang-` 12/12, `metric-` 13/13 (`v4` ile aynı).

## Bulunan iki sorun ve düzeltmeler (`qa/engine.rs`)

1. **Hitap kısa cevapta düşüyordu.** Son talimattaki "When you address me, call me X" koşul
   gibi okunuyordu: tek cümlelik görev cevabında model hitabı atlıyordu. Yeni hali
   "In your answer, call me X once." (`write_instruction`). Sistem istemindeki "not in every
   sentence" kaldı: hitap cevap başına bir kez geçiyor, her cümleye yayılmıyor.
2. **Siz ayarı İngilizce cevapları bozuyordu.** `voice()` sistem istemine dil gözetmeden
   "In Turkish, speak to the user as siz" ekliyordu. İngilizce soruda model bu yüzden notu
   kullanmayı bırakıp "could not find" diyordu (PR #12'deki sen cümlesine benzer bir yan etki).
   Cümle sistem isteminden çıkarıldı. Sen/siz artık yalnız cevap Türkçeyken son talimatta
   söyleniyor. Siz uyumu düşmedi: 10 siz vakasının hepsinde "sen" biçimi yok.

Varsayılan istem (ad ve hitap boş, sen) değişmedi: `voice()` ve `write_instruction()` bu
ayarda öncekiyle bayt bayt aynı metni üretiyor.

## Açık kalanlar

- İngilizce soruda Türkçe "bugün" notunu ara sıra kaçırma persona'dan bağımsız bir sorun
  (`persona-en-today-default` 4/5).
- `persona-ad-siz` v2'de "Ben Atlas. Ayberk Bey." diye bitti. Hitap cümleye bağlanmadan
  sona eklenmiş; denetim geçiyor ama cümle doğal değil.
- `persona-siz-ekle`: görev başlığı "Annemi aramayı ekle" olarak kaydediliyor (komut sözcüğü
  başlıkta kalıyor). Bu bir persona sorunu değil.
