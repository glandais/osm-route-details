//! Lecture de l'export du banc : CSV RFC 4180 écrit à la main, gzip en flux, document `network_edges` en serde.
//!
//! Les données sont **privées** : rien d'ici n'est journalisé ni copié ailleurs qu'en mémoire.

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

// --- Document `network_edges` : dans la bibliothèque partagée -----------------------------------------

pub use ord_graph_match::doc::{parse_document, type_code, ParsedEdge, OW_NAMES, TYPES, WK_NAMES};

/// Une ligne de l'export.
#[derive(Debug)]
pub struct ParsedRow {
    pub team_id: u64,
    pub ty: u8,
    pub excluded: bool,
    /// `None` : document absent ou illisible.
    pub edges: Option<Vec<ParsedEdge>>,
}

/// Colonnes de l'export, repérées par leur nom d'en-tête.
#[derive(Clone, Copy, Debug)]
pub struct Columns {
    team: usize,
    surface: usize,
    excluded: usize,
    doc: usize,
}

impl Columns {
    pub fn from_header(h: &[Vec<u8>]) -> anyhow::Result<Self> {
        let find = |name: &str| {
            h.iter()
                .position(|c| c.as_slice() == name.as_bytes())
                .ok_or_else(|| anyhow::anyhow!("colonne {name} absente de l'en-tête de l'export"))
        };
        Ok(Self {
            team: find("team_id")?,
            surface: find("surface_type")?,
            excluded: find("excluded_from_generator")?,
            doc: find("network_edges")?,
        })
    }

    pub fn needed(&self) -> usize {
        self.team.max(self.surface).max(self.excluded).max(self.doc) + 1
    }
}

/// Interprète une ligne ; `None` si l'identifiant d'équipe est illisible ou hors de `wanted` (vide : toutes).
pub fn parse_row(cols: &Columns, fields: &[Vec<u8>], wanted: &[u64]) -> Option<ParsedRow> {
    if fields.len() < cols.needed() {
        return None;
    }
    let team_id: u64 = std::str::from_utf8(&fields[cols.team]).ok()?.trim().parse().ok()?;
    if !wanted.is_empty() && !wanted.contains(&team_id) {
        return None;
    }
    let ty = type_code(std::str::from_utf8(&fields[cols.surface]).unwrap_or(""));
    let excluded = matches!(fields[cols.excluded].as_slice(), b"t" | b"true" | b"True" | b"TRUE" | b"1");
    let edges = parse_document(&fields[cols.doc]);
    Some(ParsedRow { team_id, ty, excluded, edges })
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

    #[test]
    fn document_with_unknown_keys_and_missing_nodes() {
        let header: Vec<Vec<u8>> = [
            "team_id",
            "enable_route_planner",
            "track_id",
            "route_id",
            "surface_type",
            "excluded_from_generator",
            "distance",
            "rev",
            "network_edges",
        ]
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect();
        let cols = Columns::from_header(&header).unwrap();
        let doc = br#"{"v":1,"zzz":[1,2],"edges":[{"g":"abc","trv":12.5,"wk":"cycleway","ow":"against","fm":1.0,"tm":13.5,"inconnue":{"a":1}},{"g":"d","p":true,"fn":3,"tn":4,"wk":"zz"}]}"#;
        let row: Vec<Vec<u8>> = ["7", "t", "T", "R", "gravel", "f", "1", "2"]
            .iter()
            .map(|s| s.as_bytes().to_vec())
            .chain([doc.to_vec()])
            .collect();
        let r = parse_row(&cols, &row, &[]).unwrap();
        assert_eq!((r.team_id, r.ty, r.excluded), (7, 1, false));
        let e = r.edges.unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].wk, e[0].ow, e[0].from_node, e[0].to_node), (4, 2, -1, -1));
        assert!(e[0].len.is_nan() && e[1].trv.is_nan());
        assert_eq!((e[1].wk, e[1].from_node, e[1].to_node), (8, 3, 4));
        let mut bad = row.clone();
        bad[8] = b"{pas du json".to_vec();
        assert!(parse_row(&cols, &bad, &[]).unwrap().edges.is_none());
        bad[8] = Vec::new();
        assert!(parse_row(&cols, &bad, &[]).unwrap().edges.is_none());
        assert_eq!(type_code("autre chose"), 3);
        assert!(Columns::from_header(&header[..4]).is_err());
    }
}
