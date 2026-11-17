//! Lecture de l'export du banc : CSV (lecteur dans `ord-graph-match`), gzip en flux, document `network_edges` en serde.
//!
//! Les données sont **privées** : rien d'ici n'est journalisé ni copié ailleurs qu'en mémoire.

// --- Document `network_edges` : dans la bibliothèque partagée -----------------------------------------

pub use ord_graph_match::csv::CsvReader;
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
