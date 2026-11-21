//! GRBL `$$` settings: human-readable descriptions and parsing of the `$N=value` echoes.

/// A single GRBL setting value read from the controller (editable in the UI).
#[derive(Clone)]
pub struct Setting {
    pub num: u32,
    pub value: String,
}

/// Spanish description + unit for a GRBL 1.1 setting number.
pub fn describe(n: u32) -> &'static str {
    match n {
        0 => "Longitud del pulso de paso (µs)",
        1 => "Retardo inactivo del paso (ms)",
        2 => "Máscara de inversión de pasos",
        3 => "Máscara de inversión de dirección",
        4 => "Invertir enable de pasos (0/1)",
        5 => "Invertir pines de límite (0/1)",
        6 => "Invertir pin de sonda (0/1)",
        10 => "Máscara del informe de estado",
        11 => "Desviación de unión (mm)",
        12 => "Tolerancia de arco (mm)",
        13 => "Informe en pulgadas (0/1)",
        20 => "Límites por software (0/1)",
        21 => "Límites por hardware (0/1)",
        22 => "Ciclo de home (0/1)",
        23 => "Máscara de inversión de home",
        24 => "Avance de home (mm/min)",
        25 => "Búsqueda de home (mm/min)",
        26 => "Antirrebote de home (ms)",
        27 => "Retirada de home (mm)",
        30 => "Velocidad máx. del husillo / S máx",
        31 => "Velocidad mín. del husillo",
        32 => "Modo láser (0/1)",
        100 => "Pasos/mm en X",
        101 => "Pasos/mm en Y",
        102 => "Pasos/mm en Z",
        110 => "Velocidad máx. X (mm/min)",
        111 => "Velocidad máx. Y (mm/min)",
        112 => "Velocidad máx. Z (mm/min)",
        120 => "Aceleración X (mm/s²)",
        121 => "Aceleración Y (mm/s²)",
        122 => "Aceleración Z (mm/s²)",
        130 => "Recorrido máx. X (mm)",
        131 => "Recorrido máx. Y (mm)",
        132 => "Recorrido máx. Z (mm)",
        _ => "(parámetro)",
    }
}

/// Parse a settings echo line such as `$30=1000` or `$30=1000 (max spindle, rpm)`.
pub fn parse_line(line: &str) -> Option<(u32, String)> {
    let s = line.trim();
    let body = s.strip_prefix('$')?;
    let eq = body.find('=')?;
    let num = body[..eq].trim().parse::<u32>().ok()?;
    let rest = body[eq + 1..].trim();
    let val = rest.split_whitespace().next().unwrap_or("").to_string();
    if val.is_empty() {
        return None;
    }
    Some((num, val))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(parse_line("$30=1000"), Some((30, "1000".into())));
        assert_eq!(
            parse_line("$32=1 (laser mode, bool)"),
            Some((32, "1".into()))
        );
        assert_eq!(parse_line("$100=80.000"), Some((100, "80.000".into())));
        assert_eq!(parse_line("ok"), None);
        assert_eq!(parse_line("<Idle|MPos:0,0,0>"), None);
    }
}
