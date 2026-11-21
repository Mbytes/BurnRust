# rustburn

Aplicación nativa en **Rust** para **Linux** de diseño y control de láser (estilo
LightBurn), pensada para láseres de diodo con firmware **GRBL** como la **Elegoo Phecda
20 W**. Sin licencia, software libre.

> Estado: **MVP (fase 1)**. Flujo completo funcional: importar → capas con
> potencia/velocidad → generar G-code (vector y raster) → conectar (USB/WiFi) →
> encuadre → enviar trabajo con progreso.

## Características actuales

- **Lienzo** con zoom/pan, rejilla y límites de cama (400×400 mm por defecto).
- **Importación de SVG** (trazos vectoriales, curvas aplanadas) e **imágenes** (PNG/JPG/BMP).
- **Capas** con color, modo (Línea / Imagen), potencia %, velocidad, pasadas,
  intervalo de línea, overscan y tramado (grises / umbral / Floyd–Steinberg).
- **Generación de G-code GRBL**: corte vectorial (`M4`/`M3`, `S`, `F`) y grabado
  raster por líneas bidireccionales con overscan y modulación de potencia por píxel.
- **Conexión USB (serie)** y **WiFi (TCP)** con protocolo GRBL por conteo de
  caracteres (streaming fiable), estado en vivo (`?`), consola y jog.
- **Encuadre (framing)** del contorno a baja potencia.
- **Jog/Home/Origen**, pausa (`!`), reanudar (`~`), parada (soft-reset `0x18`).
- **Guardar/abrir proyecto** (`.rbp`, JSON) y exportar `.gcode`.

## Compilar y ejecutar

```bash
cargo run --release
```

Autocomprobación sin interfaz (genera G-code de prueba y valida):

```bash
cargo run -- --selftest
```

## Conectar la Elegoo Phecda

- **USB**: normalmente aparece como `/dev/ttyUSB0` (baudios 115200). Si no tienes
  permiso para abrir el puerto, añade tu usuario al grupo `dialout` y reinicia sesión:
  ```bash
  sudo usermod -aG dialout $USER
  ```
- **WiFi**: introduce la IP de la Phecda y el puerto TCP (por defecto 23).

La enumeración automática de puertos funciona mejor con `libudev`. Si quieres la lista
desplegable de puertos, instala `libudev-dev` y recompila con
`cargo build --release --features serialport/libudev` (opcional: siempre puedes escribir
la ruta del puerto a mano).

## Seguridad

Comprueba siempre potencia/velocidad y haz **encuadre** antes de cortar. Usa gafas de
protección adecuadas para 20 W. El origen y el tamaño de cama deben coincidir con tu
máquina (ajústalos en el panel **Máquina**).

## Hoja de ruta (próximas fases)

- Edición de nodos, texto y fuentes, operaciones booleanas.
- Previsualización/simulación del recorrido con tiempos estimados.
- Cámara y alineación.
- Biblioteca de materiales y empaquetado `.AppImage` / `.deb`.

## Licencia

MIT OR Apache-2.0.
