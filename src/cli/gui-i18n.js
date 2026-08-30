// Injected into gui.html after `GUI_LOCALE` and `GUI_T` (from bm-rayfish.json).
function t(key, vars) {
  let s = GUI_T[key] ?? key;
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      s = s.replaceAll("{" + k + "}", String(v));
    }
  }
  return s;
}
function applyStaticI18n() {
  const htmlLang =
    GUI_LOCALE === "zh-TW" ? "zh-Hant-TW" :
    GUI_LOCALE === "zh-CN" ? "zh-Hans-CN" :
    GUI_LOCALE === "ja" ? "ja" :
    GUI_LOCALE === "en" ? "en" :
    String(GUI_LOCALE || "en").replace("_", "-");
  document.documentElement.lang = htmlLang;
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    el.textContent = t(el.getAttribute("data-i18n"));
  });
  document.querySelectorAll("[data-i18n-html]").forEach((el) => {
    el.innerHTML = t(el.getAttribute("data-i18n-html"));
  });
  document.querySelectorAll("[data-i18n-placeholder]").forEach((el) => {
    el.setAttribute("placeholder", t(el.getAttribute("data-i18n-placeholder")));
  });
}
function roleLabel(role) {
  const key = "role_" + String(role || "member").toLowerCase();
  return GUI_T[key] || role;
}
function connKindLabel(kind) {
  if (kind === "direct") return t("conn_direct");
  if (kind === "relay") return t("conn_relay");
  return t("conn_idle");
}
