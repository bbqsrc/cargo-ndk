include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

pub fn calculate_distance(x: f64, y: f64) -> f64 {
    unsafe { sqrt(x * x + y * y) }
}

pub fn calculate_sine(angle: f64) -> f64 {
    unsafe { sin(angle) }
}

pub fn power(base: f64, exponent: f64) -> f64 {
    unsafe { pow(base, exponent) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distance() {
        let result = calculate_distance(3.0, 4.0);
        assert!((result - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_sine() {
        let result = calculate_sine(0.0);
        assert!(result.abs() < f64::EPSILON);
    }

    #[test]
    fn test_power() {
        let result = power(2.0, 3.0);
        assert!((result - 8.0).abs() < f64::EPSILON);
    }
}