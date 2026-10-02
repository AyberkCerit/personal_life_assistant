const dict = {
  tr: {
    "welcome.title": "PLA'ya hoş geldin",
    "welcome.body": "Notlarının durduğu klasörü seç. Var olan bir Obsidian kasası da olabilir; hiçbir dosya taşınmaz ya da değiştirilmez.",
    "welcome.open": "Vault klasörü aç",
    "tree.label": "Notlar",
    "tree.newNote": "Yeni not adı + Enter",
    "editor.empty": "Soldan bir not seç ya da yeni not oluştur.",
    "editor.readOnly": "Bu not UTF-8 değil; bozulmaması için salt okunur açıldı.",
    "conflict.message": "Bu not PLA dışında değiştirildi; senin değişikliklerin kaydedilmedi.",
    "conflict.takeExternal": "Dış sürümü al",
    "conflict.keepMine": "Benimkini kopya olarak kaydet",
    "missing.message": "Bu not PLA dışında silindi ya da taşındı; değişikliklerin kaydedilmedi.",
    "status.model.not_installed": "Model kurulu değil",
    "status.model.off": "Model kapalı",
    "status.model.running": "Model çalışıyor",
    "status.queued": "kuyrukta",
    "status.busy": "çıkarım yapılıyor…",
    "status.added": "öğe eklendi",
    "error.generic": "Bir hata oldu",
    "side.placeholder": "Görev paneli bir sonraki sürümde (F4b) burada olacak.",
  },
  en: {
    "welcome.title": "Welcome to PLA",
    "welcome.body": "Choose the folder that holds your notes. An existing Obsidian vault works too; no file is moved or changed.",
    "welcome.open": "Open vault folder",
    "tree.label": "Notes",
    "tree.newNote": "New note name + Enter",
    "editor.empty": "Pick a note on the left or create a new one.",
    "editor.readOnly": "This note is not UTF-8; it is open read-only so it is not damaged.",
    "conflict.message": "This note was changed outside PLA; your changes were not saved.",
    "conflict.takeExternal": "Take the external version",
    "conflict.keepMine": "Keep mine as a copy",
    "missing.message": "This note was deleted or moved outside PLA; your changes were not saved.",
    "status.model.not_installed": "Model not installed",
    "status.model.off": "Model off",
    "status.model.running": "Model running",
    "status.queued": "queued",
    "status.busy": "extracting…",
    "status.added": "items added",
    "error.generic": "Something went wrong",
    "side.placeholder": "The task panel arrives here in the next version (F4b).",
  },
} as const;

export type Key = keyof typeof dict.tr;
export const lang: "tr" | "en" = navigator.language.toLowerCase().startsWith("tr") ? "tr" : "en";

export function t(key: Key): string {
  return dict[lang][key];
}
