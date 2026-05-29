//! Compile-time translation tables for common keys (TextKey, TooltipKey, MonthKey).
//!
//! Formerly in uzor::i18n — now owned by mlc.
//! Layout: `TABLE[variant_index][lang_index]`.
//! - Column 0 = En (never empty — mandatory).
//! - Column 1 = Ru.
//! - Columns 2-14 = `""` stub → fallback to En at runtime.

use super::lang::N_LANG;

// =============================================================================
// TextKey table  (38 variants × N_LANG)
// =============================================================================
//
//  Col:  0=En              1=Ru                   2=Es  3=De  4=Fr  5=Pt  6=Zh  7=Ja  8=Ko  9=Ar  10=It  11=Tr  12=Pl  13=Uk  14=Hi

pub(super) static TEXT_KEY_TABLE: [[&str; N_LANG]; 38] = [
    /* 0  Delete     */ [ "Delete",      "Удалить",        "Eliminar",    "Löschen",     "Supprimer",   "Excluir",     "删除",   "削除",     "삭제",   "حذف",       "Elimina",    "Sil",       "Usuń",       "Видалити",       "हटाएं"      ],
    /* 1  Clone      */ [ "Clone",       "Клонировать",    "Clonar",      "Klonen",      "Cloner",      "Clonar",      "克隆",   "クローン",  "복제",   "استنساخ",   "Clona",      "Klonla",    "Klonuj",     "Клонувати",      "क्लोन करें"  ],
    /* 2  Copy       */ [ "Copy",        "Копировать",     "Copiar",      "Kopieren",    "Copier",      "Copiar",      "复制",   "コピー",   "복사",   "نسخ",       "Copia",      "Kopyala",   "Kopiuj",     "Копіювати",      "कॉपी करें"  ],
    /* 3  Cancel     */ [ "Cancel",      "Отмена",         "Cancelar",    "Abbrechen",   "Annuler",     "Cancelar",    "取消",   "キャンセル", "취소",   "إلغاء",     "Annulla",    "İptal",     "Anuluj",     "Скасувати",      "रद्द करें"  ],
    /* 4  Apply      */ [ "Apply",       "Применить",      "Aplicar",     "Anwenden",    "Appliquer",   "Aplicar",     "应用",   "適用",     "적용",   "تطبيق",     "Applica",    "Uygula",    "Zastosuj",   "Застосувати",    "लागू करें"  ],
    /* 5  Save       */ [ "Save",        "Сохранить",      "Guardar",     "Speichern",   "Enregistrer", "Salvar",      "保存",   "保存",     "저장",   "حفظ",       "Salva",      "Kaydet",    "Zapisz",     "Зберегти",       "सहेजें"     ],
    /* 6  Reset      */ [ "Reset",       "Сбросить",       "Restablecer", "Zurücksetzen","Réinitialiser","Redefinir",  "重置",   "リセット",  "초기화",  "إعادة تعيين","Reimposta",  "Sıfırla",   "Resetuj",    "Скинути",        "रीसेट करें" ],
    /* 7  Close      */ [ "Close",       "Закрыть",        "Cerrar",      "Schließen",   "Fermer",      "Fechar",      "关闭",   "閉じる",   "닫기",   "إغلاق",     "Chiudi",     "Kapat",     "Zamknij",    "Закрити",        "बंद करें"   ],
    /* 8  Ok         */ [ "OK",          "ОК",             "Aceptar",     "OK",          "OK",          "OK",          "确定",   "OK",       "확인",   "موافق",     "OK",         "Tamam",     "OK",         "ОК",             "ठीक है"     ],
    /* 9  Yes        */ [ "Yes",         "Да",             "Sí",          "Ja",          "Oui",         "Sim",         "是",    "はい",     "예",    "نعم",       "Sì",         "Evet",      "Tak",        "Так",            "हाँ"        ],
    /* 10 No         */ [ "No",          "Нет",            "No",          "Nein",        "Non",         "Não",         "否",    "いいえ",   "아니요",  "لا",        "No",         "Hayır",     "Nie",        "Ні",             "नहीं"       ],
    /* 11 Show       */ [ "Show",        "Показать",       "Mostrar",     "Anzeigen",    "Afficher",    "Mostrar",     "显示",   "表示",     "표시",   "إظهار",     "Mostra",     "Göster",    "Pokaż",      "Показати",       "दिखाएं"     ],
    /* 12 Hide       */ [ "Hide",        "Скрыть",         "Ocultar",     "Ausblenden",  "Masquer",     "Ocultar",     "隐藏",   "非表示",   "숨기기",  "إخفاء",     "Nascondi",   "Gizle",     "Ukryj",      "Сховати",        "छिपाएं"     ],
    /* 13 Lock       */ [ "Lock",        "Заблокировать",  "Bloquear",    "Sperren",     "Verrouiller", "Bloquear",    "锁定",   "ロック",   "잠금",   "قفل",       "Blocca",     "Kilitle",   "Zablokuj",   "Заблокувати",    "लॉक करें"   ],
    /* 14 Unlock     */ [ "Unlock",      "Разблокировать", "Desbloquear", "Entsperren",  "Déverrouiller","Desbloquear","解锁",   "ロック解除", "잠금해제", "فتح القفل",  "Sblocca",    "Kilidi aç", "Odblokuj",   "Розблокувати",   "अनलॉक करें" ],
    /* 15 Enable     */ [ "Enable",      "Включить",       "Habilitar",   "Aktivieren",  "Activer",     "Ativar",      "启用",   "有効化",   "활성화",  "تفعيل",     "Abilita",    "Etkinleştir","Włącz",     "Увімкнути",      "सक्षम करें" ],
    /* 16 Disable    */ [ "Disable",     "Отключить",      "Deshabilitar","Deaktivieren","Désactiver",  "Desativar",   "禁用",   "無効化",   "비활성화", "تعطيل",     "Disabilita", "Devre dışı bırak","Wyłącz","Вимкнути",  "अक्षम करें" ],
    /* 17 Settings   */ [ "Settings",    "Настройки",      "Ajustes",     "Einstellungen","Paramètres",  "Configurações","设置",  "設定",     "설정",   "الإعدادات", "Impostazioni","Ayarlar",   "Ustawienia", "Налаштування",   "सेटिंग्स"   ],
    /* 18 Properties */ [ "Properties",  "Свойства",       "Propiedades", "Eigenschaften","Propriétés",  "Propriedades","属性",   "プロパティ", "속성",   "الخصائص",   "Proprietà",  "Özellikler","Właściwości","Властивості",    "गुण"        ],
    /* 19 Color      */ [ "Color",       "Цвет",           "Color",       "Farbe",       "Couleur",     "Cor",         "颜色",   "色",       "색상",   "اللون",     "Colore",     "Renk",      "Kolor",      "Колір",          "रंग"        ],
    /* 20 Style      */ [ "Style",       "Стиль",          "Estilo",      "Stil",        "Style",       "Estilo",      "样式",   "スタイル",  "스타일",  "النمط",     "Stile",      "Stil",      "Styl",       "Стиль",          "शैली"       ],
    /* 21 Width      */ [ "Width",       "Ширина",         "Ancho",       "Breite",      "Largeur",     "Largura",     "宽度",   "幅",       "너비",   "العرض",     "Larghezza",  "Genişlik",  "Szerokość",  "Ширина",         "चौड़ाई"    ],
    /* 22 Opacity    */ [ "Opacity",     "Прозрачность",   "Opacidad",    "Deckkraft",   "Opacité",     "Opacidade",   "不透明度", "不透明度",  "불투명도", "التعتيم",   "Opacità",    "Opaklık",   "Nieprzezroczystość","Непрозорість","अपारदर्शिता"],
    /* 23 Background */ [ "Background",  "Фон",            "Fondo",       "Hintergrund", "Arrière-plan","Plano de fundo","背景",  "背景",     "배경",   "الخلفية",   "Sfondo",     "Arka plan", "Tło",        "Фон",            "पृष्ठभूमि"  ],
    /* 24 Foreground */ [ "Foreground",  "Передний план",  "Primer plano","Vordergrund", "Premier plan","Primeiro plano","前景",  "前景",     "전경",   "المقدمة",   "Primo piano","Ön plan",   "Pierwsz. plan","Передній план","अग्रभूमि"  ],
    /* 25 Border     */ [ "Border",      "Граница",        "Borde",       "Rahmen",      "Bordure",     "Borda",       "边框",   "ボーダー",  "테두리",  "الحدود",    "Bordo",      "Kenarlık",  "Obramowanie","Межа",           "बॉर्डर"     ],
    /* 26 Text       */ [ "Text",        "Текст",          "Texto",       "Text",        "Texte",       "Texto",       "文本",   "テキスト",  "텍스트",  "النص",      "Testo",      "Metin",     "Tekst",      "Текст",          "पाठ"        ],
    /* 27 Font       */ [ "Font",        "Шрифт",          "Fuente",      "Schriftart",  "Police",      "Fonte",       "字体",   "フォント",  "글꼴",   "الخط",      "Carattere",  "Yazı tipi", "Czcionka",   "Шрифт",          "फ़ॉन्ट"     ],
    /* 28 Size       */ [ "Size",        "Размер",         "Tamaño",      "Größe",       "Taille",      "Tamanho",     "大小",   "サイズ",   "크기",   "الحجم",     "Dimensione", "Boyut",     "Rozmiar",    "Розмір",         "आकार"       ],
    /* 29 Left       */ [ "Left",        "Слева",          "Izquierda",   "Links",       "Gauche",      "Esquerda",    "左",    "左",       "왼쪽",   "يسار",      "Sinistra",   "Sol",       "Lewo",       "Ліворуч",        "बाएं"       ],
    /* 30 Right      */ [ "Right",       "Справа",         "Derecha",     "Rechts",      "Droite",      "Direita",     "右",    "右",       "오른쪽",  "يمين",      "Destra",     "Sağ",       "Prawo",      "Праворуч",       "दाएं"       ],
    /* 31 Top        */ [ "Top",         "Сверху",         "Arriba",      "Oben",        "Haut",        "Topo",        "顶部",   "上",       "위",    "أعلى",      "Alto",       "Üst",       "Góra",       "Вгорі",          "ऊपर"        ],
    /* 32 Bottom     */ [ "Bottom",      "Снизу",          "Abajo",       "Unten",       "Bas",         "Baixo",       "底部",   "下",       "아래",   "أسفل",      "Basso",      "Alt",       "Dół",        "Внизу",          "नीचे"       ],
    /* 33 Center     */ [ "Center",      "По центру",      "Centro",      "Mitte",       "Centre",      "Centro",      "居中",   "中央",     "가운데",  "وسط",       "Centro",     "Merkez",    "Środek",     "По центру",      "केंद्र"     ],
    /* 34 Back       */ [ "Back",        "Назад",          "Atrás",       "Zurück",      "Retour",      "Voltar",      "返回",   "戻る",     "뒤로",   "رجوع",      "Indietro",   "Geri",      "Wstecz",     "Назад",          "वापस"       ],
    /* 35 Add        */ [ "Add",         "Добавить",       "Añadir",      "Hinzufügen",  "Ajouter",     "Adicionar",   "添加",   "追加",     "추가",   "إضافة",     "Aggiungi",   "Ekle",      "Dodaj",      "Додати",         "जोड़ें"     ],
    /* 36 Loading    */ [ "Loading...",  "Загрузка...",    "Cargando...","Laden...",     "Chargement...","Carregando...","加载中…", "読込中…",  "불러오는 중…","جارٍ التحميل…","Caricamento…","Yükleniyor…","Ładowanie…","Завантаження…","लोड हो रहा है…"],
    /* 37 Disconnect */ [ "Disconnect",  "Отключить",      "Desconectar", "Trennen",     "Déconnecter", "Desconectar", "断开",   "切断",     "연결 끊기","قطع الاتصال", "Disconnetti","Bağlantıyı kes","Rozłącz","Відключити",    "डिस्कनेक्ट" ],
];

// =============================================================================
// TooltipKey table  (10 variants × N_LANG)
// =============================================================================
//
//  Col:  0=En                    1=Ru                       2..14=""

pub(super) static TOOLTIP_KEY_TABLE: [[&str; N_LANG]; 10] = [
    /* 0  CloseWindow */ [ "Close window",       "Закрыть окно",         "Cerrar ventana",    "Fenster schließen",  "Fermer la fenêtre",  "Fechar janela",      "关闭窗口",  "ウィンドウを閉じる","창 닫기",    "إغلاق النافذة",    "Chiudi finestra",    "Pencereyi kapat",    "Zamknij okno",       "Закрити вікно",       "विंडो बंद करें"   ],
    /* 1  CloseApp    */ [ "Quit application",   "Закрыть приложение",   "Salir",             "Anwendung beenden",  "Quitter l'application","Sair do aplicativo","退出应用",  "アプリを終了",     "앱 종료",    "إنهاء التطبيق",    "Esci dall'app",      "Uygulamadan çık",    "Zamknij aplikację",  "Закрити додаток",     "ऐप बंद करें"      ],
    /* 2  Minimize    */ [ "Minimize",           "Свернуть",             "Minimizar",         "Minimieren",         "Réduire",            "Minimizar",          "最小化",    "最小化",           "최소화",     "تصغير",            "Minimizza",          "Küçült",             "Minimalizuj",        "Згорнути",            "छोटा करें"        ],
    /* 3  Maximize    */ [ "Maximize",           "Развернуть",           "Maximizar",         "Maximieren",         "Agrandir",           "Maximizar",          "最大化",    "最大化",           "최대화",     "تكبير",            "Massimizza",         "Büyüt",              "Maksymalizuj",       "Розгорнути",          "बड़ा करें"        ],
    /* 4  Restore     */ [ "Restore",            "Восстановить",         "Restaurar",         "Wiederherstellen",   "Restaurer",          "Restaurar",          "还原",      "元に戻す",         "복원",       "استعادة",          "Ripristina",         "Geri yükle",         "Przywróć",           "Відновити",           "पुनर्स्थापित करें"],
    /* 5  NewWindow   */ [ "New window",         "Новое окно",           "Nueva ventana",     "Neues Fenster",      "Nouvelle fenêtre",   "Nova janela",        "新建窗口",  "新しいウィンドウ", "새 창",      "نافذة جديدة",      "Nuova finestra",     "Yeni pencere",       "Nowe okno",          "Нове вікно",          "नई विंडो"         ],
    /* 6  Menu        */ [ "Menu",               "Меню",                 "Menú",              "Menü",               "Menu",               "Menu",               "菜单",      "メニュー",         "메뉴",       "القائمة",          "Menu",               "Menü",               "Menu",               "Меню",                "मेनू"             ],
    /* 7  NewTab      */ [ "New tab",            "Новая вкладка",        "Nueva pestaña",     "Neuer Tab",          "Nouvel onglet",      "Nova aba",           "新建标签",  "新しいタブ",       "새 탭",      "علامة تبويب جديدة","Nuova scheda",       "Yeni sekme",         "Nowa karta",         "Нова вкладка",        "नया टैब"          ],
    /* 8  CloseTab    */ [ "Close tab",          "Закрыть вкладку",      "Cerrar pestaña",    "Tab schließen",      "Fermer l'onglet",    "Fechar aba",         "关闭标签",  "タブを閉じる",     "탭 닫기",    "إغلاق علامة التبويب","Chiudi scheda",    "Sekmeyi kapat",      "Zamknij kartę",      "Закрити вкладку",     "टैब बंद करें"     ],
    /* 9  Undo        */ [ "Undo",               "Отменить",             "Deshacer",          "Rückgängig",         "Annuler",            "Desfazer",           "撤销",      "元に戻す",         "실행 취소",  "تراجع",            "Annulla",            "Geri al",            "Cofnij",             "Скасувати",           "पूर्ववत करें"    ],
];

// =============================================================================
// MonthKey tables  (12 months × N_LANG)
// =============================================================================
//
//  Col:  0=En     1=Ru     2..14=""

pub(super) static MONTH_TABLE_SHORT: [[&str; N_LANG]; 12] = [
    /* 0  January   */ [ "Jan", "Янв", "Ene", "Jan", "janv.", "jan.", "1月",  "1月",  "1월",  "يناير",   "gen.", "Oca", "sty", "Січ", "जन."  ],
    /* 1  February  */ [ "Feb", "Фев", "Feb", "Feb", "févr.", "fev.", "2月",  "2月",  "2월",  "فبراير",  "feb.", "Şub", "lut", "Лют", "फर."  ],
    /* 2  March     */ [ "Mar", "Мар", "Mar", "Mär", "mars",  "mar.", "3月",  "3月",  "3월",  "مارس",    "mar.", "Mar", "mar", "Бер", "मार्च"],
    /* 3  April     */ [ "Apr", "Апр", "Abr", "Apr", "avr.",  "abr.", "4月",  "4月",  "4월",  "أبريل",   "apr.", "Nis", "kwi", "Кві", "अप्र." ],
    /* 4  May       */ [ "May", "Май", "May", "Mai", "mai",   "mai",  "5月",  "5月",  "5월",  "مايو",    "mag.", "May", "maj", "Тра", "मई"   ],
    /* 5  June      */ [ "Jun", "Июн", "Jun", "Jun", "juin",  "jun.",  "6月",  "6月",  "6월",  "يونيو",   "giu.", "Haz", "cze", "Чер", "जून"  ],
    /* 6  July      */ [ "Jul", "Июл", "Jul", "Jul", "juil.", "jul.",  "7月",  "7月",  "7월",  "يوليو",   "lug.", "Tem", "lip", "Лип", "जुल." ],
    /* 7  August    */ [ "Aug", "Авг", "Ago", "Aug", "août",  "ago.", "8月",  "8月",  "8월",  "أغسطس",   "ago.", "Ağu", "sie", "Сер", "अग."  ],
    /* 8  September */ [ "Sep", "Сен", "Sep", "Sep", "sept.", "set.", "9月",  "9月",  "9월",  "سبتمبر",  "set.", "Eyl", "wrz", "Вер", "सित." ],
    /* 9  October   */ [ "Oct", "Окт", "Oct", "Okt", "oct.",  "out.", "10月", "10月", "10월", "أكتوبر",  "ott.", "Eki", "paź", "Жов", "अक्ट."],
    /* 10 November  */ [ "Nov", "Ноя", "Nov", "Nov", "nov.",  "nov.", "11月", "11月", "11월", "نوفمبر",  "nov.", "Kas", "lis", "Лис", "नव."  ],
    /* 11 December  */ [ "Dec", "Дек", "Dic", "Dez", "déc.",  "dez.", "12月", "12月", "12월", "ديسمبر",  "dic.", "Ara", "gru", "Гру", "दिस." ],
];

pub(super) static MONTH_TABLE_FULL: [[&str; N_LANG]; 12] = [
    /* 0  January   */ [ "January",   "Январь",   "Enero",      "Januar",    "Janvier",    "Janeiro",    "一月",   "1月",      "1월",   "يناير",    "Gennaio",   "Ocak",     "Styczeń",    "Січень",    "जनवरी"    ],
    /* 1  February  */ [ "February",  "Февраль",  "Febrero",    "Februar",   "Février",    "Fevereiro",  "二月",   "2月",      "2월",   "فبراير",   "Febbraio",  "Şubat",    "Luty",       "Лютий",     "फरवरी"    ],
    /* 2  March     */ [ "March",     "Март",     "Marzo",      "März",      "Mars",       "Março",      "三月",   "3月",      "3월",   "مارس",     "Marzo",     "Mart",     "Marzec",     "Березень",  "मार्च"    ],
    /* 3  April     */ [ "April",     "Апрель",   "Abril",      "April",     "Avril",      "Abril",      "四月",   "4月",      "4월",   "أبريل",    "Aprile",    "Nisan",    "Kwiecień",   "Квітень",   "अप्रैल"   ],
    /* 4  May       */ [ "May",       "Май",      "Mayo",       "Mai",       "Mai",        "Maio",       "五月",   "5月",      "5월",   "مايو",     "Maggio",    "Mayıs",    "Maj",        "Травень",   "मई"       ],
    /* 5  June      */ [ "June",      "Июнь",     "Junio",      "Juni",      "Juin",       "Junho",      "六月",   "6月",      "6월",   "يونيو",    "Giugno",    "Haziran",  "Czerwiec",   "Червень",   "जून"      ],
    /* 6  July      */ [ "July",      "Июль",     "Julio",      "Juli",      "Juillet",    "Julho",      "七月",   "7月",      "7월",   "يوليو",    "Luglio",    "Temmuz",   "Lipiec",     "Липень",    "जुलाई"    ],
    /* 7  August    */ [ "August",    "Август",   "Agosto",     "August",    "Août",       "Agosto",     "八月",   "8月",      "8월",   "أغسطس",    "Agosto",    "Ağustos",  "Sierpień",   "Серпень",   "अगस्त"    ],
    /* 8  September */ [ "September", "Сентябрь", "Septiembre", "September", "Septembre",  "Setembro",   "九月",   "9月",      "9월",   "سبتمبر",   "Settembre", "Eylül",    "Wrzesień",   "Вересень",  "सितंबर"   ],
    /* 9  October   */ [ "October",   "Октябрь",  "Octubre",    "Oktober",   "Octobre",    "Outubro",    "十月",   "10月",     "10월",  "أكتوبر",   "Ottobre",   "Ekim",     "Październik","Жовтень",   "अक्टूबर"  ],
    /* 10 November  */ [ "November",  "Ноябрь",   "Noviembre",  "November",  "Novembre",   "Novembro",   "十一月",  "11月",     "11월",  "نوفمبر",   "Novembre",  "Kasım",    "Listopad",   "Листопад",  "नवंबर"    ],
    /* 11 December  */ [ "December",  "Декабрь",  "Diciembre",  "Dezember",  "Décembre",   "Dezembro",   "十二月",  "12月",     "12월",  "ديسمبر",   "Dicembre",  "Aralık",   "Grudzień",   "Грудень",   "दिसंबर"   ],
];

// =============================================================================
// Completeness invariant (executable documentation)
// =============================================================================
//
// See `tables.rs` completeness test — same rule: no empty cell, so a missing
// translation can't silently fall back to English unnoticed.
#[cfg(test)]
mod completeness {
    use super::*;

    fn assert_full(name: &str, rows: &[[&str; N_LANG]]) {
        for (r, row) in rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                assert!(
                    !cell.is_empty(),
                    "{name}: empty translation at row {r}, language column {c}"
                );
            }
        }
    }

    #[test]
    fn every_common_table_is_fully_translated() {
        assert_full("TEXT_KEY_TABLE", &TEXT_KEY_TABLE);
        assert_full("TOOLTIP_KEY_TABLE", &TOOLTIP_KEY_TABLE);
        assert_full("MONTH_TABLE_SHORT", &MONTH_TABLE_SHORT);
        assert_full("MONTH_TABLE_FULL", &MONTH_TABLE_FULL);
    }
}
