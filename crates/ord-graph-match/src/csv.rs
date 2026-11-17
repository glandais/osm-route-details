//! Lecteur CSV RFC 4180 écrit à la main, pour l'export du banc : l'outil `ord-graph-overlay` et le banc de
//! vraisemblance de `ord-loops-bench` le partagent. Les données lues sont **privées** : rien d'ici n'est journalisé.

use std::io::{self, BufRead};

/// Lecteur CSV : une ligne = une liste de champs en octets. Guillemets doublés, séparateurs et sauts de ligne
/// dans les champs entre guillemets, CRLF ou LF, dernière ligne sans saut de ligne.
#[derive(Debug)]
pub struct CsvReader<R: BufRead> {
    r: R,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum St {
    FieldStart,
    Plain,
    Quoted,
    /// Un guillemet vient d'être vu dans un champ entre guillemets : doublé, ou fermant.
    QuoteSeen,
}

impl<R: BufRead> CsvReader<R> {
    pub fn new(r: R) -> Self {
        Self { r }
    }

    /// Lit une ligne dans `fields` (tampons réutilisés) ; `false` en fin de flux.
    pub fn read_record(&mut self, fields: &mut Vec<Vec<u8>>) -> io::Result<bool> {
        let mut n = 0usize;
        Self::reset(fields, n);
        let mut st = St::FieldStart;
        let mut saw_any = false;
        loop {
            let buf = self.r.fill_buf()?;
            if buf.is_empty() {
                break;
            }
            saw_any = true;
            let (mut i, mut done) = (0usize, false);
            while i < buf.len() {
                match st {
                    St::Quoted => match buf[i..].iter().position(|&b| b == b'"') {
                        Some(k) => {
                            fields[n].extend_from_slice(&buf[i..i + k]);
                            i += k + 1;
                            st = St::QuoteSeen;
                        }
                        None => {
                            fields[n].extend_from_slice(&buf[i..]);
                            i = buf.len();
                        }
                    },
                    St::QuoteSeen => {
                        if buf[i] == b'"' {
                            fields[n].push(b'"');
                            i += 1;
                            st = St::Quoted;
                        } else {
                            st = St::Plain; // guillemet fermant ; l'octet est relu comme texte ordinaire
                        }
                    }
                    St::FieldStart | St::Plain => {
                        let c = buf[i];
                        i += 1;
                        match c {
                            b'"' if st == St::FieldStart => st = St::Quoted,
                            b',' => {
                                n += 1;
                                Self::reset(fields, n);
                                st = St::FieldStart;
                            }
                            b'\n' => {
                                done = true;
                                break;
                            }
                            b'\r' => {}
                            _ => {
                                fields[n].push(c);
                                st = St::Plain;
                            }
                        }
                    }
                }
            }
            self.r.consume(i);
            if done {
                fields.truncate(n + 1);
                return Ok(true);
            }
        }
        if !saw_any {
            return Ok(false);
        }
        fields.truncate(n + 1);
        Ok(true)
    }

    fn reset(fields: &mut Vec<Vec<u8>>, n: usize) {
        if fields.len() <= n {
            fields.resize_with(n + 1, Vec::new);
        }
        fields[n].clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_all(text: &[u8], cap: usize) -> Vec<Vec<String>> {
        let mut r = CsvReader::new(io::BufReader::with_capacity(cap, text));
        let mut out = Vec::new();
        let mut f = Vec::new();
        while r.read_record(&mut f).unwrap() {
            out.push(f.iter().map(|x| String::from_utf8(x.clone()).unwrap()).collect());
        }
        out
    }

    #[test]
    fn csv_quotes_empty_fields_and_last_line_without_lf() {
        let text = b"a,b,c\n1,\"x \"\"q\"\" y\",\n\"l1\nl2\",,3\r\n4,5,6";
        for cap in [1usize, 2, 3, 5, 7, 64] {
            let rows = read_all(text, cap.max(1));
            assert_eq!(
                rows,
                vec![
                    vec!["a", "b", "c"],
                    vec!["1", "x \"q\" y", ""],
                    vec!["l1\nl2", "", "3"],
                    vec!["4", "5", "6"]
                ],
                "tampon {cap}"
            );
        }
        assert!(read_all(b"", 8).is_empty());
        assert_eq!(read_all(b"x\n", 8), vec![vec!["x"]]);
    }
}
