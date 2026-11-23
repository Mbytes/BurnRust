//! Minimal UI internationalisation. Spanish strings in the source are the keys;
//! `tr` returns the English translation when the language is English, or the
//! Spanish key unchanged otherwise (so any untranslated string degrades to
//! Spanish). Keys and values are `'static`, so `tr` borrows nothing from `self`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    Es,
    En,
}

impl Default for Lang {
    fn default() -> Self {
        Lang::Es
    }
}

impl Lang {
    pub fn label(self) -> &'static str {
        match self {
            Lang::Es => "Español",
            Lang::En => "English",
        }
    }
}

/// Translate a Spanish source string to the active language.
pub fn tr(lang: Lang, es: &'static str) -> &'static str {
    if lang == Lang::Es {
        return es;
    }
    en(es)
}

/// Spanish → English dictionary for the main menus/panels. Unknown strings fall
/// back to the Spanish key.
fn en(es: &'static str) -> &'static str {
    match es {
        // Toolbar — projects / files
        "🗋 Nuevo proyecto" => "🗋 New project",
        "📂 Abrir proyecto" => "📂 Open project",
        "💾 Guardar proyecto" => "💾 Save project",
        "➕ Importar SVG" => "➕ Import SVG",
        "🖼 Importar imagen" => "🖼 Import image",
        "📄 Importar PDF" => "📄 Import PDF",
        "✒ Como vectores (corte)" => "✒ As vectors (cut)",
        "🖼 Como imagen (grabado)" => "🖼 As image (engrave)",
        "⚙ Generar G-code" => "⚙ Generate G-code",
        "⬇ Guardar .nc" => "⬇ Save .nc",
        // Toolbar — toggles
        "▦ Rejilla" => "▦ Grid",
        "🧲 Rejilla" => "🧲 Grid",
        "🎯 Snap objetos" => "🎯 Snap objects",
        "👁 Recorrido" => "👁 Toolpath",
        "🔥 Test material" => "🔥 Material test",
        "✛ Print&Cut" => "✛ Print&Cut",
        "🛠 Configuración" => "🛠 Settings",
        "⚙ Ajustes GRBL" => "⚙ GRBL settings",
        "❓ Ayuda" => "❓ Help",
        "Cámara calibrada" => "Calibrated camera",
        "Idioma" => "Language",

        // Panel headings
        "Herramientas" => "Tools",
        "Operaciones" => "Operations",
        "Capas" => "Layers",
        "Objeto" => "Object",
        "Máquina" => "Machine",
        "Materiales" => "Materials",
        "Cámara / foto" => "Camera / photo",
        "Láser (GRBL)" => "Laser (GRBL)",

        // Tools
        "🖱 Seleccionar" => "🖱 Select",
        "✎ Nodos" => "✎ Nodes",
        "▭ Rectángulo" => "▭ Rectangle",
        "⬭ Elipse" => "⬭ Ellipse",
        "／ Línea" => "／ Line",
        "⬡ Polígono" => "⬡ Polygon",
        "📈 Polilínea" => "📈 Polyline",
        "🅣 Texto" => "🅣 Text",
        "📏 Medir" => "📏 Measure",

        // Operations / arrange
        "∪ Unir" => "∪ Union",
        "− Restar" => "− Subtract",
        "∩ Intersecar" => "∩ Intersect",
        "⊕ Soldar" => "⊕ Weld",
        "⟳ Offset" => "⟳ Offset",
        "⊏ Cerrar trazado" => "⊏ Close path",
        "✂ Recortar a la cama" => "✂ Clip to bed",
        "〰 Suavizar nodos" => "〰 Smooth nodes",
        "⌁ Vectorizar imagen" => "⌁ Trace image",
        "▦ Matriz…" => "▦ Array…",
        "◯ Circular" => "◯ Circular",
        "⬌ Espejo H" => "⬌ Mirror H",
        "⬍ Espejo V" => "⬍ Mirror V",
        "⟳ 90°" => "⟳ 90°",
        "⧉ Duplicar" => "⧉ Duplicate",
        "🗑 Eliminar" => "🗑 Delete",
        "✓ Terminar" => "✓ Finish",

        // Layers panel
        "➕ Añadir capa" => "➕ Add layer",
        "＋ Guardar capa" => "＋ Save layer",
        "Visible" => "Visible",
        "Salida" => "Output",
        "Modo:" => "Mode:",
        "Línea" => "Line",
        "Imagen" => "Image",
        "Potencia %" => "Power %",
        "Potencia mín %" => "Min power %",
        "Velocidad" => "Speed",
        "Pasadas" => "Passes",
        "Intervalo" => "Interval",
        "Overscan" => "Overscan",
        "Tramado:" => "Dither:",
        "Grises" => "Greyscale",
        "Umbral" => "Threshold",
        "F-S" => "F-S",
        "Atkinson" => "Atkinson",
        "Jarvis" => "Jarvis",
        "Bayer" => "Bayer",

        // Object panel
        "Nombre:" => "Name:",
        "X (mm)" => "X (mm)",
        "Y (mm)" => "Y (mm)",
        "Ancho (mm)" => "Width (mm)",
        "Alto (mm)" => "Height (mm)",
        "Rotación" => "Rotation",
        "Versión" => "Version",
        "Capa:" => "Layer:",
        "Ningún objeto seleccionado." => "No object selected.",
        "Ajustes de imagen" => "Image adjustments",
        "Brillo" => "Brightness",
        "Contraste" => "Contrast",
        "Gamma" => "Gamma",
        "Invertir" => "Invert",
        "Vectorizar (trazar)" => "Trace (vectorise)",
        "Suavizado" => "Smoothing",

        // Text tool
        "Texto:" => "Text:",
        "Fuente:" => "Font:",
        "Automática" => "Automatic",
        "Altura:" => "Height:",
        "Numeración / texto variable (serie)" => "Numbering / variable text (series)",

        // Machine panel / config
        "Cama X (mm)" => "Bed X (mm)",
        "Cama Y (mm)" => "Bed Y (mm)",
        "Ancho de cama X (mm)" => "Bed width X (mm)",
        "Alto de cama Y (mm)" => "Bed height Y (mm)",
        "S máx ($30)" => "S max ($30)",
        "Baudios (USB)" => "Baud rate (USB)",
        "Velocidad de viaje (mm/min)" => "Travel speed (mm/min)",
        "Velocidad viaje" => "Travel speed",
        "Modo láser:" => "Laser mode:",
        "M4 dinámico" => "M4 dynamic",
        "M3" => "M3",
        "Origen:" => "Origin:",
        "Frontal-izq" => "Front-left",
        "Superior-izq" => "Top-left",
        "Optimizar recorrido" => "Optimize travel",
        "Optimizar recorrido (menos viajes)" => "Optimize travel (fewer moves)",
        "Fondo blanco (lienzo claro)" => "White background (light canvas)",
        "🔍 Ajustar vista a la cama" => "🔍 Fit view to bed",
        "Perfil de máquina:" => "Machine profile:",
        "Mis láseres:" => "My lasers:",
        "💾 Guardar como láser nuevo" => "💾 Save as new laser",
        "Nombre del láser" => "Laser name",
        "🛠 Configuración de la máquina" => "🛠 Machine settings",

        // Laser / connection panel
        "🔌 Conectar" => "🔌 Connect",
        "Desconectar" => "Disconnect",
        "Enviar" => "Send",
        "Conectar" => "Connect",
        "Puerto / host" => "Port / host",
        "▶ Iniciar" => "▶ Start",
        "⏸ Pausa" => "⏸ Pause",
        "▶ Reanudar" => "▶ Resume",
        "⏹ Parar" => "⏹ Stop",
        "🏠 Home" => "🏠 Home",
        "⊹ Origen aquí" => "⊹ Set origin here",
        "▣ Encuadre" => "▣ Framing",
        "Consola" => "Console",
        "Limpiar" => "Clear",
        "Comando GRBL (p.ej. $$, G0 X10)…" => "GRBL command (e.g. $$, G0 X10)…",

        // Camera panel
        "📷 Cargar foto" => "📷 Load photo",
        "📸 Capturar cámara USB" => "📸 Capture USB camera",
        "🗑 Quitar foto" => "🗑 Remove photo",
        "↺ Sin distorsión" => "↺ No distortion",
        "Distorsión de lente" => "Lens distortion",

        // Print & Cut
        "✛ Añadir marcas de registro" => "✛ Add registration marks",
        "📐 Calcular alineación" => "📐 Compute alignment",
        "Aplicar alineación al corte" => "Apply alignment to the cut",

        // Buttons / misc
        "✅ Aplicar" => "✅ Apply",
        "⚙ Generar test → G-code" => "⚙ Generate test → G-code",
        "↺ Restablecer ajustes" => "↺ Reset adjustments",
        "Horizontal" => "Horizontal",
        "Vertical" => "Vertical",
        "USB" => "USB",
        "WiFi" => "WiFi",

        // Tools / operations extra
        "Shift+clic: añadir operandos" => "Shift+click: add operands",
        "Funde contornos solapados en uno (ideal para soldar letras de texto cursivo)" =>
            "Merge overlapping contours into one (ideal for welding cursive text letters)",
        "Elimina la geometría fuera del área de trabajo" => "Remove geometry outside the work area",
        "Redondea las esquinas con curvas tipo bézier (spline Catmull-Rom) a través de los nodos" =>
            "Round corners with bézier-like curves (Catmull-Rom spline) through the nodes",
        "Cierra los trazados abiertos del objeto seleccionado" => "Close the open paths of the selected object",
        "Convierte los trazos del PDF a vectores (pdftocairo/inkscape)." =>
            "Converts the PDF's strokes to vectors (pdftocairo/inkscape).",
        "Rasteriza la 1ª página; luego puedes vectorizarla (poppler/ghostscript)." =>
            "Rasterises the first page; you can then trace it (poppler/ghostscript).",
        "Vaciar y empezar de cero (Ctrl+Z para deshacer)" => "Clear and start over (Ctrl+Z to undo)",
        "Marcas de registro y alineación de corte sobre impresión" =>
            "Registration marks and cut alignment over a print",
        "Ajustar a la cuadrícula" => "Snap to the grid",
        "Ajustar bordes/centros a otros objetos y a la cama" =>
            "Snap edges/centres to other objects and the bed",
        "Disposición:" => "Layout:",

        // Text tool hints
        "Lados:" => "Sides:",

        // Object panel extra
        "Capa" => "Layer",
        "Convierte la imagen en contornos de corte" => "Converts the image into cut contours",
        "Invertir (trazar lo claro)" => "Invert (trace the light areas)",
        "Traza los bordes a este umbral y crea un objeto vectorial" =>
            "Traces the edges at this threshold and creates a vector object",
        "Mantener proporción" => "Keep aspect ratio",
        "Organizar" => "Arrange",
        "🔗 Agrupar" => "🔗 Group",
        "Ctrl+G · agrupa la selección (Shift+clic añade objetos)" =>
            "Ctrl+G · group the selection (Shift+click adds objects)",
        "✂ Desagrupar" => "✂ Ungroup",
        "Alinear:" => "Align:",
        "Izquierda" => "Left",
        "Centro X" => "Centre X",
        "Derecha" => "Right",
        "Arriba" => "Top",
        "Centro Y" => "Centre Y",
        "Abajo" => "Bottom",
        "Distribuir:" => "Distribute:",
        "Espaciado horizontal uniforme" => "Uniform horizontal spacing",
        "Espaciado vertical uniforme" => "Uniform vertical spacing",
        "Duplicar en rejilla o círculo" => "Duplicate in a grid or circle",

        // Config window extra
        "Aplicar este láser (tamaño, origen, modo, baudios…)" =>
            "Apply this laser (size, origin, mode, baud…)",
        "Borrar este láser" => "Delete this laser",
        "Guarda la configuración actual con este nombre" => "Save the current configuration under this name",
        "Los presets ajustan tamaño de cama y S máx. «Guardar como láser nuevo» guarda toda la configuración actual (origen, modo, baudios…) y queda en «Mis láseres». Verifica siempre con tu máquina." =>
            "Presets set the bed size and S max. \"Save as new laser\" stores the whole current configuration (origin, mode, baud…) and keeps it under \"My lasers\". Always verify against your machine.",

        // Camera panel extra
        "Dispositivo:" => "Device:",
        "Captura un fotograma del dispositivo V4L2 con ffmpeg" => "Capture a frame from the V4L2 device with ffmpeg",
        "🎯 Calibrar (4 esquinas)" => "🎯 Calibrate (4 corners)",
        "… clic en 4 esquinas" => "… click the 4 corners",
        "Mostrar superposición" => "Show overlay",
        "Opacidad" => "Opacity",
        "Corrige barril (−) o cojín (+)" => "Corrects barrel (−) or pincushion (+)",
        "Carga una foto del tablero para alinear el diseño por perspectiva." =>
            "Load a photo of the board to align the design by perspective.",
        "Marca la esquina" => "Mark corner",
        "de 4" => "of 4",

        // Materials panel extra
        "▶ aplica a la capa del objeto seleccionado" => "▶ applies to the selected object's layer",
        "Nombre nuevo" => "New name",

        // Laser / connection panel extra
        "Refrescar puertos" => "Refresh ports",
        "Puerto:" => "Port:",
        "Baudios:" => "Baud rate:",
        "Host:" => "Host:",
        "Puerto TCP:" => "TCP port:",
        "Mover (jog)" => "Move (jog)",
        "Paso:" => "Step:",
        "Avance:" => "Feed:",
        "⌂ Home" => "⌂ Home",
        "⦿ Origen" => "⦿ Origin",
        "Salida del trabajo" => "Job output",
        "Cortar solo gráficos seleccionados" => "Cut only selected graphics",
        "Usar origen de la pieza (coords. relativas)" => "Use part origin (relative coords.)",
        "Origen del trabajo:" => "Job origin:",
        "Encuadre (framing)" => "Framing",
        "▭ Trazar" => "▭ Trace",
        "No hay diseño para encuadrar" => "No design to frame",
        "⏵ Reanudar" => "⏵ Resume",
        "Enviando trabajo" => "Sending job",
        "líneas" => "lines",
        "📏 Medir: clic en 2 puntos" => "📏 Measure: click 2 points",

        // Material test window
        "🔥 Test de material" => "🔥 Material test",
        "Columnas (potencia)" => "Columns (power)",
        "Filas (velocidad)" => "Rows (speed)",
        "Potencia mín/máx %" => "Min/max power %",
        "Velocidad mín/máx" => "Min/max speed",
        "Celda / separación (mm)" => "Cell / spacing (mm)",
        "Intervalo relleno (mm)" => "Fill interval (mm)",
        "Origen X / Y (mm)" => "Origin X / Y (mm)",
        "Luego: Guardar .nc o Iniciar (conectado)." => "Then: Save .nc or Start (when connected).",

        // GRBL settings window
        "⚙ Ajustes GRBL ($$)" => "⚙ GRBL settings ($$)",
        "🔄 Leer ($$)" => "🔄 Read ($$)",
        "💾 Escribir todos" => "💾 Write all",
        "(conéctate a la Phecda para leer/escribir)" => "(connect to the Phecda to read/write)",
        "Sin datos. Pulsa «Leer ($$)» con la máquina conectada." =>
            "No data. Press \"Read ($$)\" with the machine connected.",
        "Escribir este parámetro" => "Write this parameter",

        // Array window
        "▦ Matriz / Array" => "▦ Matrix / Array",
        "Duplica la selección en rejilla o en círculo." => "Duplicate the selection in a grid or a circle.",
        "Copias" => "Copies",
        "Radio (mm)" => "Radius (mm)",
        "Centro: el radio por encima de la selección." => "Centre: the radius above the selection.",
        "Columnas" => "Columns",
        "Filas" => "Rows",
        "Separación X (mm)" => "Spacing X (mm)",
        "Separación Y (mm)" => "Spacing Y (mm)",

        // Print & Cut window
        "✛ Print & Cut" => "✛ Print & Cut",
        "Corta sobre una impresión ya existente alineando el láser a marcas de registro." =>
            "Cut over an existing print by aligning the laser to registration marks.",
        "1) Añade marcas y expórtalas/imprímelas con el diseño." =>
            "1) Add marks and export/print them with the design.",
        "2) Coloca la hoja impresa en la cama." => "2) Place the printed sheet on the bed.",
        "3) Haz jog del cabezal a cada marca y anota su X/Y de máquina." =>
            "3) Jog the head to each mark and note its machine X/Y.",
        "4) Calcula la alineación y genera el corte." => "4) Compute the alignment and generate the cut.",
        "Sin marcas todavía." => "No marks yet.",
        "Marcas: diseño → medida (máquina)" => "Marks: design → measured (machine)",
        "Diseño (mm)" => "Design (mm)",
        "X máquina" => "Machine X",
        "Y máquina" => "Machine Y",
        "La alineación se aplica al generar G-code." => "The alignment is applied when generating G-code.",
        "Aún sin alineación calculada." => "No alignment computed yet.",

        // Help window
        "BurnRust — control láser GRBL" => "BurnRust — GRBL laser control",
        "Aplicación nativa para láseres de diodo (Elegoo Phecda y compatibles GRBL 1.1)." =>
            "Native app for diode lasers (Elegoo Phecda and GRBL 1.1 compatibles).",
        "Lienzo" => "Canvas",
        "• Rueda del ratón: zoom alrededor del cursor." => "• Mouse wheel: zoom around the cursor.",
        "• Arrastrar con botón central: desplazar la vista." => "• Drag with the middle button: pan the view.",
        "• Reglas y cuadrícula en mm (líneas cada 10 mm, mayor cada 50)." =>
            "• Rulers and grid in mm (lines every 10 mm, major every 50).",
        "• Seleccionar: mover, escalar (tiradores) y rotar objetos." =>
            "• Select: move, scale (handles) and rotate objects.",
        "• Nodos: editar puntos (doble clic añade/elimina)." =>
            "• Nodes: edit points (double-click adds/removes).",
        "• Rectángulo, Elipse, Línea, Polígono, Polilínea, Texto." =>
            "• Rectangle, Ellipse, Line, Polygon, Polyline, Text.",
        "• Operaciones: Unir, Restar, Intersecar, Soldar, Offset (Shift+clic añade operandos)." =>
            "• Operations: Union, Subtract, Intersect, Weld, Offset (Shift+click adds operands).",
        "Atajos de teclado" => "Keyboard shortcuts",
        "• Ctrl+Z / Ctrl+Y: deshacer / rehacer." => "• Ctrl+Z / Ctrl+Y: undo / redo.",
        "• Ctrl+D: duplicar selección." => "• Ctrl+D: duplicate selection.",
        "• Ctrl+G / Ctrl+Shift+G: agrupar / desagrupar." => "• Ctrl+G / Ctrl+Shift+G: group / ungroup.",
        "• Supr: eliminar selección." => "• Del: delete selection.",
        "Organizar (panel Objeto)" => "Arrange (Object panel)",
        "• Agrupar/Desagrupar; Espejo H/V; Rotar 90°." => "• Group/Ungroup; Mirror H/V; Rotate 90°.",
        "• Alinear (6 direcciones), Distribuir H/V, Matriz (rejilla/círculo)." =>
            "• Align (6 directions), Distribute H/V, Array (grid/circle).",
        "• Shift+clic: añadir objetos a la selección." => "• Shift+click: add objects to the selection.",
        "Edición avanzada" => "Advanced editing",
        "• Snap objetos: al mover, ajusta bordes/centros a otros objetos y a la cama." =>
            "• Snap objects: while moving, snap edges/centres to other objects and the bed.",
        "• Operaciones: Cerrar trazado, Recortar a la cama, Suavizar nodos (bézier)." =>
            "• Operations: Close path, Clip to bed, Smooth nodes (bézier).",
        "• Nodos (✎): arrastra para mover, doble clic borra un nodo o inserta en un tramo." =>
            "• Nodes (✎): drag to move, double-click deletes a node or inserts on a segment.",
        "• Numeración/texto variable: plantilla con # (p.ej. 'Pieza ###')." =>
            "• Numbering/variable text: template with # (e.g. 'Part ###').",
        "• Simulación del recorrido: ▶/⏸/⏮, barra de progreso y velocidad." =>
            "• Toolpath simulation: ▶/⏸/⏮, progress bar and speed.",
        "Cámara y Print&Cut" => "Camera and Print&Cut",
        "• Cámara USB en vivo: captura un fotograma (ffmpeg) y calíbralo (4 esquinas)." =>
            "• Live USB camera: capture a frame (ffmpeg) and calibrate it (4 corners).",
        "• Corrección de distorsión de lente (k1/k2) sobre la foto." =>
            "• Lens distortion correction (k1/k2) on the photo.",
        "• Print&Cut (✛): añade marcas de registro, mide su posición y alinea el corte." =>
            "• Print&Cut (✛): add registration marks, measure their position and align the cut.",
        "Capas y materiales" => "Layers and materials",
        "• Cada capa tiene modo (Línea/Imagen), potencia %, velocidad y pasadas." =>
            "• Each layer has a mode (Line/Image), power %, speed and passes.",
        "• Materiales: presets aplicables a la capa del objeto seleccionado." =>
            "• Materials: presets applicable to the selected object's layer.",
        "Grabado de imagen" => "Image engraving",
        "• Ajustes: brillo, contraste, gamma, invertir." => "• Adjustments: brightness, contrast, gamma, invert.",
        "• Tramado: Grises, Umbral, Floyd-Steinberg, Atkinson, Jarvis, Bayer." =>
            "• Dithering: Greyscale, Threshold, Floyd-Steinberg, Atkinson, Jarvis, Bayer.",
        "• Vectorizar (⌁): traza la imagen a un umbral y crea contornos de corte." =>
            "• Trace (⌁): trace the image at a threshold and create cut contours.",
        "Máquina (GRBL)" => "Machine (GRBL)",
        "• Conecta por USB (/dev/ttyUSB0) o WiFi (host:puerto)." =>
            "• Connect via USB (/dev/ttyUSB0) or WiFi (host:port).",
        "• Jog, Home, fijar Origen, Encuadre (framing) a baja potencia." =>
            "• Jog, Home, set Origin, Framing at low power.",
        "• Ajustes GRBL: leer/escribir parámetros $$." => "• GRBL settings: read/write $$ parameters.",
        "• Configuración 🛠: tamaño de cama X/Y y perfiles de otras máquinas." =>
            "• Settings 🛠: bed size X/Y and profiles for other machines.",
        "Flujo recomendado" => "Recommended workflow",
        "1. Diseña o importa (SVG/imagen). 2. Asigna capa y parámetros." =>
            "1. Design or import (SVG/image). 2. Assign a layer and parameters.",
        "3. Genera G-code y revisa el recorrido/tiempo. 4. Conecta, encuadra y envía." =>
            "3. Generate G-code and review the toolpath/time. 4. Connect, frame and send.",
        "⚠ Seguridad: usa gafas adecuadas, ventilación y nunca dejes el láser sin vigilancia." =>
            "⚠ Safety: use proper glasses, ventilation and never leave the laser unattended.",

        _ => es,
    }
}
