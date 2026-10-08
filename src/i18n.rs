//! Языки интерфейса: русский, английский, испанский.
//! В коде строки пишутся по-русски и проходят через t() — она подставляет перевод.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// 0 — русский, 1 — английский, 2 — испанский.
static LANG: AtomicU8 = AtomicU8::new(0);

pub const NAMES: [&str; 3] = ["Русский", "English", "Español"];

pub fn get() -> u8 {
    LANG.load(Ordering::Relaxed)
}

pub fn set(l: u8) {
    LANG.store(l.min(2), Ordering::Relaxed);
}

#[link(name = "kernel32")]
extern "system" {
    fn GetUserDefaultUILanguage() -> u16;
}

/// Язык Windows: русский/украинский/белорусский → русский, испанский → испанский, иначе английский.
pub fn system() -> u8 {
    match unsafe { GetUserDefaultUILanguage() } & 0x3FF {
        0x19 | 0x22 | 0x23 => 0,
        0x0A => 2,
        _ => 1,
    }
}

/// Выбрать язык: сохранённый или, если не выбран, язык Windows.
pub fn init(saved: Option<u8>) {
    set(saved.unwrap_or_else(system));
}

/// Перевод строки.
pub fn t(ru: &'static str) -> &'static str {
    let l = get();
    if l == 0 {
        return ru;
    }
    match table().get(ru) {
        Some((en, es)) => if l == 1 { en } else { es },
        None => ru,
    }
}

/// Перевод шаблона с {} и подстановка значений по порядку.
pub fn tf(ru: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::new();
    let mut it = args.iter();
    let mut rest = t(ru);
    while let Some(p) = rest.find("{}") {
        out.push_str(&rest[..p]);
        if let Some(a) = it.next() {
            out.push_str(&a.to_string());
        }
        rest = &rest[p + 2..];
    }
    out.push_str(rest);
    out
}

fn table() -> &'static HashMap<&'static str, (&'static str, &'static str)> {
    static T: OnceLock<HashMap<&'static str, (&'static str, &'static str)>> = OnceLock::new();
    T.get_or_init(|| WORDS.iter().map(|(r, e, s)| (*r, (*e, *s))).collect())
}

const WORDS: &[(&str, &str, &str)] = &[
    // вкладки
    ("Подсветка", "Lighting", "Iluminación"),
    ("Производительность", "Performance", "Rendimiento"),
    ("Настройки", "Settings", "Ajustes"),
    // режимы питания
    ("Энергосбережение", "Power saver", "Ahorro de energía"),
    ("Тихий", "Quiet", "Silencioso"),
    ("Баланс", "Balanced", "Equilibrado"),
    ("Максимум", "Maximum", "Máximo"),
    ("Дольше без зарядки. Мощность и шум — минимум.", "Longer on battery. Minimum power and noise.", "Más batería. Potencia y ruido al mínimo."),
    ("Почти не слышно. Браузер, кино, работа.", "Barely audible. Browsing, movies, work.", "Casi inaudible. Navegar, películas, trabajo."),
    ("Обычный режим на каждый день.", "The everyday mode.", "El modo para el día a día."),
    ("Больше мощности для игр. Вентиляторы громче.", "More power for games. Louder fans.", "Más potencia para juegos. Ventiladores más fuertes."),
    ("Вся мощность. Вентиляторы на полную.", "Full power. Fans at full speed.", "Toda la potencia. Ventiladores al máximo."),
    ("Дольше без зарядки, мощность и шум — минимум", "Longer on battery, minimum power and noise", "Más batería, potencia y ruido al mínimo"),
    ("Почти не слышно: браузер, кино, работа", "Barely audible: browsing, movies, work", "Casi inaudible: navegar, películas, trabajo"),
    ("Обычный режим на каждый день", "The everyday mode", "El modo para el día a día"),
    ("Больше мощности для игр, вентиляторы громче", "More power for games, louder fans", "Más potencia para juegos, ventiladores más fuertes"),
    ("Вся мощность, вентиляторы на полную", "Full power, fans at full speed", "Toda la potencia, ventiladores al máximo"),
    ("Режим", "Mode", "Modo"),
    ("Значок батареи в трее", "Battery icon in the tray", "Icono de batería en la bandeja"),
    ("Левый клик по значку — «Энергосбережение» и обратно «Баланс» (в этом режиме на значке листик). Правый — все режимы. Крестик закрывает окно, а программа продолжает работать в трее.",
     "Left click on the icon switches to Power saver and back to Balanced (a leaf shows on the icon in that mode). Right click shows all modes. The close button closes the window, the program keeps running in the tray.",
     "Clic izquierdo en el icono: Ahorro de energía y de vuelta a Equilibrado (en ese modo aparece una hoja). Clic derecho: todos los modos. La X cierra la ventana y el programa sigue en la bandeja."),
    ("Мощность", "Power", "Potencia"),
    ("Шум", "Noise", "Ruido"),
    ("Нет связи с ноутбуком: программе нужны права администратора.", "No connection to the laptop: the program needs administrator rights.", "Sin conexión con el portátil: el programa necesita permisos de administrador."),
    ("Процессор", "CPU", "Procesador"),
    ("Видеокарта", "GPU", "Gráfica"),
    ("нагрузка {}%", "load {}%", "carga {}%"),
    ("об/мин · стоит", "RPM · stopped", "RPM · parado"),
    ("об/мин", "RPM", "RPM"),
    ("Вентилятор {}", "Fan {}", "Ventilador {}"),
    ("Температура за 2 минуты", "Temperature, last 2 minutes", "Temperatura, últimos 2 minutos"),
    ("● видеокарта", "● GPU", "● gráfica"),
    ("● процессор", "● CPU", "● procesador"),
    ("собираю данные…", "collecting data…", "recopilando datos…"),
    // автозапуск
    ("{} Автозапуск включён — работает всегда.", "{} Autostart is on — always works.", "{} El inicio automático está activado: funciona siempre."),
    ("{} Работает, только пока программа запущена — после перезагрузки сам не включится.", "{} Works only while the program is running — it won't turn on by itself after a restart.", "{} Solo funciona mientras el programa está abierto: no se activará solo tras reiniciar."),
    ("Включить автозапуск", "Turn on autostart", "Activar inicio automático"),
    ("Автозапуск", "Autostart", "Inicio automático"),
    ("Запускать вместе с Windows — тихо, в фоне", "Start with Windows — quietly, in the background", "Iniciar con Windows, en silencio y en segundo plano"),
    ("Зачем это нужно. Свои цвета, обычные эффекты (дыхание, спектр, волна, сканер) и режим питания хранятся в самом ноутбуке — они работают и без программы, даже если её удалить.",
     "Why you need it. Your colors, the regular effects (breathing, spectrum, wave, scanner) and the power mode are stored in the laptop itself — they work without the program, even if you delete it.",
     "Para qué sirve. Tus colores, los efectos normales (respiración, espectro, onda, escáner) y el modo de energía se guardan en el propio portátil: funcionan sin el programa, incluso si lo borras."),
    ("А вот это делает сама программа, и работает оно, только пока она запущена:", "But these are done by the program itself and work only while it is running:", "Pero esto lo hace el propio programa y solo funciona mientras está abierto:"),
    ("•  эффекты «На нажатия» (круг, крест, брызги, тепловая карта…);", "•  “On key press” effects (circle, cross, splash, heat map…);", "•  efectos «Al pulsar» (círculo, cruz, salpicadura, mapa de calor…);"),
    ("•  Fn-выключение подсветки гасит заодно эмблему, контур и кнопку питания;", "•  turning the backlight off with Fn also turns off the logo, light bar and power button;", "•  apagar la retroiluminación con Fn apaga también el logo, la barra de luz y el botón de encendido;"),
    ("•  поменянные местами функции HOME / END / DEL (правый клик по клавише);", "•  swapped HOME / END / DEL functions (right click on the key);", "•  funciones intercambiadas de HOME / END / DEL (clic derecho en la tecla);"),
    ("•  значок батареи в трее и переключение режимов из него.", "•  the battery icon in the tray and switching modes from it.", "•  el icono de batería en la bandeja y el cambio de modo desde él."),
    ("С автозапуском программа сама стартует при входе в Windows, окно не открывается. Если значок в трее выключен, она просто тихо работает в фоне — чтобы открыть окно, запусти программу ещё раз.",
     "With autostart the program starts when you sign in to Windows, without opening the window. If the tray icon is off, it just runs quietly in the background — to open the window, run the program again.",
     "Con el inicio automático el programa arranca al entrar en Windows, sin abrir la ventana. Si el icono de la bandeja está desactivado, funciona en silencio en segundo plano: para abrir la ventana, vuelve a ejecutar el programa."),
    // отладка, о программе, язык
    ("Отладка", "Debug", "Depuración"),
    ("Записывать отладку в файл на рабочем столе", "Write a debug log to a file on the desktop", "Guardar un registro de depuración en el escritorio"),
    ("Нужна, только если что-то работает не так: файл «Open Alienware Command Center_debug.txt» покажет, где проблема.",
     "Only needed if something goes wrong: the file “Open Alienware Command Center_debug.txt” will show where the problem is.",
     "Solo hace falta si algo falla: el archivo «Open Alienware Command Center_debug.txt» mostrará dónde está el problema."),
    ("О программе", "About", "Acerca de"),
    ("Лёгкая замена Alienware Command Center без телеметрии и лишних служб.", "A lightweight Alienware Command Center replacement with no telemetry or extra services.", "Un sustituto ligero de Alienware Command Center sin telemetría ni servicios innecesarios."),
    ("Язык", "Language", "Idioma"),
    // подсветка
    ("нажми на клавишу или зону · Ctrl — несколько · протяни мышью — область · правый клик по HOME, END, DEL — поменять их функции",
     "click a key or zone · Ctrl — several · drag — an area · right click on HOME, END, DEL — swap their functions",
     "pulsa una tecla o zona · Ctrl: varias · arrastra: un área · clic derecho en HOME, END, DEL: intercambiar funciones"),
    ("Num Lock выключен — цифровой блок красится своим вторым цветом", "Num Lock is off — the numpad uses its second color", "Num Lock desactivado: el teclado numérico usa su segundo color"),
    ("Выключи Num Lock, чтобы задать цифровому блоку второй цвет", "Turn Num Lock off to set a second color for the numpad", "Desactiva Num Lock para dar un segundo color al teclado numérico"),
    ("Поменянные местами HOME / END / DEL делает сама программа.", "Swapped HOME / END / DEL is done by the program itself.", "El intercambio de HOME / END / DEL lo hace el propio programa."),
    ("Быстрый выбор", "Quick select", "Selección rápida"),
    ("Выбрано", "Selected", "Seleccionado"),
    ("КОРПУС — ВИД СЗАДИ", "CHASSIS — REAR VIEW", "CHASIS — VISTA TRASERA"),
    ("Эмблема", "Logo", "Logo"),
    ("Световой контур", "Light bar", "Barra de luz"),
    ("Вся клавиатура", "Whole keyboard", "Todo el teclado"),
    ("Цифры", "Numbers", "Números"),
    ("Всё сразу", "Everything", "Todo"),
    ("Поиск устройств…", "Looking for devices…", "Buscando dispositivos…"),
    ("Клавиатура: {} · Корпус: {}", "Keyboard: {} · Chassis: {}", "Teclado: {} · Chasis: {}"),
    ("подключена", "connected", "conectado"),
    ("не найдена", "not found", "no encontrado"),
    ("подключён", "connected", "conectado"),
    ("не найден", "not found", "no encontrado"),
    ("Нажми, чтобы скопировать", "Click to copy", "Haz clic para copiar"),
    ("ничего — нажми на клавишу или зону", "nothing — click a key or zone", "nada: pulsa una tecla o zona"),
    ("зон: {}", "zones: {}", "zonas: {}"),
    ("Эффект", "Effect", "Efecto"),
    ("На нажатия", "On key press", "Al pulsar"),
    ("Клавиши загораются, когда нажимаешь.", "Keys light up when you press them.", "Las teclas se iluminan al pulsarlas."),
    ("Этот эффект рисует сама программа, а не клавиатура.", "This effect is drawn by the program, not the keyboard.", "Este efecto lo dibuja el programa, no el teclado."),
    ("Под эффектом — свои цвета", "Your colors under the effect", "Tus colores bajo el efecto"),
    ("На всю клавиатуру. Свои цвета не пропадут.", "On the whole keyboard. Your colors won't be lost.", "En todo el teclado. Tus colores no se perderán."),
    ("Скорость", "Speed", "Velocidad"),
    ("Медленно", "Slow", "Lenta"),
    ("Средне", "Medium", "Media"),
    ("Быстро", "Fast", "Rápida"),
    ("Направление", "Direction", "Dirección"),
    ("Цвет эффекта", "Effect color", "Color del efecto"),
    ("Цвет", "Color", "Color"),
    ("Мои цвета", "My colors", "Mis colores"),
    ("ПКМ — убрать", "Right click — remove", "Clic derecho: quitar"),
    ("Добавить текущий цвет", "Add the current color", "Añadir el color actual"),
    ("Яркость выбранного", "Brightness of selection", "Brillo de la selección"),
    ("У этого эффекта цвета меняются сами. Нажми на эмблему или контур, чтобы задать их цвет.", "This effect changes colors by itself. Click the logo or light bar to set their color.", "Este efecto cambia los colores solo. Pulsa el logo o la barra de luz para elegir su color."),
    ("Название набора", "Preset name", "Nombre del conjunto"),
    ("Сохранить", "Save", "Guardar"),
    ("Набор {}", "Preset {}", "Conjunto {}"),
    ("Наборы", "Presets", "Conjuntos"),
    ("Пока нет сохранённых наборов", "No saved presets yet", "Aún no hay conjuntos guardados"),
    ("Сохранить набор", "Save preset", "Guardar conjunto"),
    ("Скоро", "Soon", "Pronto"),
    // эффекты
    ("Нажатая клавиша", "Pressed key", "Tecla pulsada"),
    ("Круг от нажатия", "Ripple", "Onda"),
    ("Крест", "Cross", "Cruz"),
    ("Лучи", "Rays", "Rayos"),
    ("Брызги радугой", "Rainbow splash", "Salpicadura arcoíris"),
    ("Брызги цветом", "Color splash", "Salpicadura de color"),
    ("Тепловая карта", "Heat map", "Mapa de calor"),
    ("Дыхание", "Breathing", "Respiración"),
    ("Спектр", "Spectrum", "Espectro"),
    ("Радужная волна", "Rainbow wave", "Onda arcoíris"),
    ("Сканер", "Scanner", "Escáner"),
    ("Свои цвета", "Your colors", "Tus colores"),
    // названия клавиш
    ("Пробел", "Space", "Espacio"),
    ("Микрофон", "Microphone", "Micrófono"),
    ("Без звука", "Mute", "Silencio"),
    ("Тише", "Volume down", "Bajar volumen"),
    ("Громче", "Volume up", "Subir volumen"),
    // трей
    ("Заряд {}% · {} · режим «{}»", "Battery {}% · {} · mode “{}”", "Batería {}% · {} · modo «{}»"),
    ("Заряд {}% · {}", "Battery {}% · {}", "Batería {}% · {}"),
    ("от сети", "plugged in", "conectado"),
    ("от батареи", "on battery", "con batería"),
    ("Хватит примерно на {} ч {} мин", "About {} h {} min left", "Quedan unas {} h {} min"),
    ("Питание подключено", "Charger connected", "Cargador conectado"),
    ("Оцениваю время работы…", "Estimating time left…", "Calculando el tiempo restante…"),
    ("сейчас", "now", "ahora"),
    ("Открыть программу", "Open the program", "Abrir el programa"),
    ("Выход", "Exit", "Salir"),
];
