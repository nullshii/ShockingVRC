const STORAGE_KEY = "svrc-lang";

const DICT = {
  en: {
    title: "ShockingVRC Remote",
    promptCmd: "remote",
    navLang: "RU",
    navTheme: "Theme",
    stopBtn: "STOP",
    heroTitle: "ShockingVRC<br>Remote",
    heroSub: "Encrypted peer-to-peer partner control panel · protocol 2. Commands travel straight to your partner's device; this server only introduces you.",
    connectHeading: "Connect",
    connectDesc: "Enter the 16-character control code your partner gave you, or paste their <code>shockingvrc://</code> invite link.",
    fieldCode: "Code or link",
    placeholderCode: "XXXX-XXXX-XXXX-XXXX or shockingvrc://2/…",
    btnPaste: "Paste",
    fieldPass: "Passphrase",
    placeholderPass: "shared separately",
    btnShow: "Show",
    btnHide: "Hide",
    fieldName: "Your name",
    placeholderName: "shown to your partner",
    btnConnect: "Connect ->",
    btnCancel: "Cancel",
    modeCode: "CONTROL CODE",
    modeCodePartial: "CODE",
    modeLink: "INVITE LINK",
    modeLinkPass: "ENCRYPTED LINK",
    stepSignal: "1. Signal",
    stepP2P: "2. P2P",
    stepKeys: "3. Verify",
    infoHeading: "How it works & shortcuts",
    infoP2PLbl: "transport",
    infoP2PVal: "Direct WebRTC data channel with DTLS + Ed25519 key verification.",
    infoSafeLbl: "safety",
    infoSafeVal: "Partner device caps are always enforced locally. Press Esc anytime to stop & disconnect.",
    infoKeysLbl: "hotkeys",
    infoKeysVal: "Esc — emergency stop · Space — pause all channels · T — theme · R — language.",
    infoZeroLbl: "privacy",
    infoZeroVal: "Zero analytics, zero external requests, no server logging of commands.",
    sessionPartner: "Partner",
    sessionLatency: "Latency",
    sessionSecurity: "Security",
    verifyCode: "Connected with a control code",
    verifyLink: "Partner fingerprint {fp}",
    verifyLinkNoSig: "Partner fingerprint {fp} (signature check unavailable in this browser)",
    btnPauseAll: "Stop all channels",
    hintControl: "Turn on runs for the set time (press again to stop early). Hold fires only while pressed. Space stops all channels; STOP or Esc ends the session.",
    noDevicesHeading: "No devices yet",
    noDevicesBody: "Your partner hasn't shared a device. Its channels appear here as soon as they connect one.",
    deviceSlot: "device {n}",
    channelTitle: "Channel {name}",
    lockedByPartner: "Locked by partner",
    btnTurnOn: "Turn on · {s} s",
    btnStopLeft: "Stop · {left} s left",
    btnHold: "Hold",
    btnHolding: "Firing…",
    lblHz: "Frequency",
    lblInt: "Intensity",
    lblPower: "Power",
    lblSecs: "Time",
    metaOut: "out {out} / cap {cap}",
    feedbackMore: "{partner} asks for MORE",
    feedbackLess: "{partner} asks for LESS",
    footerLeft1: "ShockingVRC Partner Remote",
    footerLeft2: "Protocol 2 · WebRTC P2P",
    footerRight1: "Esc: emergency stop",
    footerRight2: "T: theme · R: language",
    statusFinding: "Finding your partner…",
    statusP2P: "Connecting peer to peer…",
    statusKeys: "Checking each other's keys…",
    statusPending: "Waiting for your partner to accept…",
    statusDeriving: "Deriving key from passphrase…",
    toastConnected: "Connected",
    toastStopped: "Session stopped",
    toastPausedAll: "All channels stopped",
    toastHiddenTab: "Keep this tab open — some browsers throttle hidden tabs",
    toastPasted: "Pasted from clipboard",
    toastPasteFail: "Clipboard access denied — paste manually",
    toastCancelled: "Connection cancelled",
    errWrongRole: "This link lets them control you — open it in the ShockingVRC app",
    errCodeLen: "A control code has 16 characters",
    errReachServer: "Cannot reach this signaling server",
    errNoRoom: "Nobody is waiting under that code",
    errRateLimited: "Too many attempts — wait a little",
    errBusy: "The partner is busy",
    errP2PTimeout: "Could not reach your partner directly or by relay",
    errConnFailed: "The peer-to-peer connection failed",
    errNoControl: "This invite does not let you control them",
    errPartnerSilent: "The partner stopped answering",
    errConnLost: "Connection lost",
    errStoppedBy: "Stopped by {partner}: {reason}",
    errPartnerLeft: "{partner} left",
    errClosed: "The connection closed",
    errTimeout: "The partner took too long",
    themeTitle: "Theme",
    themeClose: "Close",
    themePalette: "Palette",
    themeSpeed: "Speed",
    themeDensity: "Density",
  },
  ru: {
    title: "ShockingVRC Remote",
    promptCmd: "remote",
    navLang: "EN",
    navTheme: "Тема",
    stopBtn: "СТОП",
    heroTitle: "ShockingVRC<br>Remote",
    heroSub: "Зашифрованная P2P-панель управления · протокол 2. Команды идут напрямую на устройство партнёра — этот сервер только знакомит вас.",
    connectHeading: "Подключение",
    connectDesc: "Введи 16-символьный код управления от партнёра или вставь ссылку <code>shockingvrc://</code>.",
    fieldCode: "Код или ссылка",
    placeholderCode: "XXXX-XXXX-XXXX-XXXX или shockingvrc://2/…",
    btnPaste: "Вставить",
    fieldPass: "Пароль",
    placeholderPass: "передаётся отдельно",
    btnShow: "Показать",
    btnHide: "Скрыть",
    fieldName: "Твоё имя",
    placeholderName: "увидит партнёр",
    btnConnect: "Подключиться ->",
    btnCancel: "Отмена",
    modeCode: "КОД УПРАВЛЕНИЯ",
    modeCodePartial: "КОД",
    modeLink: "ССЫЛКА-ИНВАЙТ",
    modeLinkPass: "ЗАШИФРОВАННАЯ ССЫЛКА",
    stepSignal: "1. Сигнал",
    stepP2P: "2. P2P",
    stepKeys: "3. Ключи",
    infoHeading: "Как это работает и клавиши",
    infoP2PLbl: "транспорт",
    infoP2PVal: "Прямой WebRTC-канал с проверкой ключей DTLS + Ed25519.",
    infoSafeLbl: "безопасность",
    infoSafeVal: "Лимиты мощности всегда проверяются на устройстве партнёра. Esc в любой момент останавливает и отключает сессию.",
    infoKeysLbl: "горячие клавиши",
    infoKeysVal: "Esc — экстренный стоп · Space — остановить каналы · T — тема · R — язык.",
    infoZeroLbl: "приватность",
    infoZeroVal: "Ноль аналитики, ноль внешних запросов, команды не проходят через сервер.",
    sessionPartner: "Партнёр",
    sessionLatency: "Задержка",
    sessionSecurity: "Защита",
    verifyCode: "Подключено по коду управления",
    verifyLink: "Отпечаток партнёра {fp}",
    verifyLinkNoSig: "Отпечаток партнёра {fp} (браузер не поддерживает проверку подписи)",
    btnPauseAll: "Остановить все каналы",
    hintControl: "Кнопка «Включить» работает заданное время (повторное нажатие останавливает). «Удерживать» активно только пока зажато. Space — пауза каналов; СТОП или Esc — завершить сессию.",
    noDevicesHeading: "Устройств пока нет",
    noDevicesBody: "Партнёр ещё не открыл доступ к устройству. Каналы появятся здесь сразу после подключения.",
    deviceSlot: "устройство {n}",
    channelTitle: "Канал {name}",
    lockedByPartner: "Заблокировано партнёром",
    btnTurnOn: "Включить · {s} с",
    btnStopLeft: "Стоп · осталось {left} с",
    btnHold: "Удерживать",
    btnHolding: "Активно…",
    lblHz: "Частота",
    lblInt: "Интенсив.",
    lblPower: "Мощность",
    lblSecs: "Время",
    metaOut: "вых {out} / лимит {cap}",
    feedbackMore: "{partner} просит СИЛЬНЕЕ",
    feedbackLess: "{partner} просит СЛАБЕЕ",
    footerLeft1: "ShockingVRC Partner Remote",
    footerLeft2: "Протокол 2 · WebRTC P2P",
    footerRight1: "Esc: экстренный стоп",
    footerRight2: "T: тема · R: язык",
    statusFinding: "Ищем партнёра…",
    statusP2P: "Устанавливаем P2P-соединение…",
    statusKeys: "Проверяем ключи…",
    statusPending: "Ждём подтверждения партнёра…",
    statusDeriving: "Вычисляем ключ из пароля…",
    toastConnected: "Подключено",
    toastStopped: "Сессия остановлена",
    toastPausedAll: "Все каналы остановлены",
    toastHiddenTab: "Не сворачивай вкладку — браузер может замедлять фоновые вкладки",
    toastPasted: "Вставлено из буфера",
    toastPasteFail: "Нет доступа к буферу — вставь вручную",
    toastCancelled: "Подключение отменено",
    errWrongRole: "Эта ссылка даёт управление тобой — открой её в приложении ShockingVRC",
    errCodeLen: "Код управления состоит из 16 символов",
    errReachServer: "Не удалось подключиться к сигнальному серверу",
    errNoRoom: "По этому коду никто не ждёт",
    errRateLimited: "Слишком много попыток — подожди немного",
    errBusy: "Партнёр занят",
    errP2PTimeout: "Не удалось связаться с партнёром напрямую или через релей",
    errConnFailed: "Сбой P2P-соединения",
    errNoControl: "Это приглашение не даёт прав на управление",
    errPartnerSilent: "Партнёр перестал отвечать",
    errConnLost: "Соединение потеряно",
    errStoppedBy: "Остановлено партнёром {partner}: {reason}",
    errPartnerLeft: "{partner} отключился",
    errClosed: "Соединение закрыто",
    errTimeout: "Партнёр слишком долго не отвечает",
    themeTitle: "Тема",
    themeClose: "Закрыть",
    themePalette: "Палитра",
    themeSpeed: "Скорость",
    themeDensity: "Плотность",
  },
};

function detectLang() {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "ru" || saved === "en") return saved;
  } catch {}
  return (navigator.language || "en").toLowerCase().startsWith("ru") ? "ru" : "en";
}

let lang = detectLang();
const listeners = new Set();

export function getLang() {
  return lang;
}

export function t(key, vars = null) {
  const str = DICT[lang]?.[key] ?? DICT.en[key] ?? key;
  if (!vars) return str;
  return str.replace(/\{(\w+)\}/g, (_, k) => (vars[k] !== undefined ? String(vars[k]) : `{${k}}`));
}

export function setLang(next) {
  if (next !== "en" && next !== "ru") return;
  lang = next;
  document.documentElement.lang = lang;
  try {
    localStorage.setItem(STORAGE_KEY, lang);
  } catch {}
  applyDomI18n();
  listeners.forEach((fn) => fn(lang));
}

export function toggleLang() {
  setLang(lang === "ru" ? "en" : "ru");
}

export function onLangChange(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function applyDomI18n() {
  document.documentElement.lang = lang;
  document.title = t("title");
  for (const el of document.querySelectorAll("[data-i18n]")) {
    el.textContent = t(el.dataset.i18n);
  }
  for (const el of document.querySelectorAll("[data-i18n-html]")) {
    el.innerHTML = t(el.dataset.i18nHtml);
  }
  for (const el of document.querySelectorAll("[data-i18n-placeholder]")) {
    el.setAttribute("placeholder", t(el.dataset.i18nPlaceholder));
  }
}
