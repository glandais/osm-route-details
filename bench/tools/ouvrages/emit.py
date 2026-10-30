import json
G=json.load(open('corpus_geom.json'))
I=json.load(open('raw/ign_results.json'))
ign={}
for p,r,l in zip(I['points'],I['rge'],I['lidar']):
    m={}
    for x in l.get('measures',[]):
        t=x['title']
        if 'tropolitaine' in t: m[t.split(' - ')[2]]=x['z']
    ign[(p[0],p[1])]=dict(rge_alti_mnt=r if r!=-99999.0 else None, lidar_mnt=m.get('MNT'), lidar_mns=m.get('MNS'), lidar_mnh=m.get('MNH'))
    if m.get('MNT')==-99999.0: ign[(p[0],p[1])]=dict(rge_alti_mnt=None,lidar_mnt=None,lidar_mns=None,lidar_mnh=None,hors_couverture=True)
J=lambda v: json.dumps(v,ensure_ascii=False)
OBS={
 'stn-acces-nord':"MNH LiDAR au milieu de la partie non taguée = 19,7 m (MNS 22,8 / MNT 3,1) : structure élevée au-dessus du sol, ce qui confirme l'absence de tag bridge.",
 'stn-pont':"MNS au milieu 33,5 m, à l'extrémité nord de la partie taguée 50,9 m (MNT ≈ 0) : le point haut du tablier n'est pas au milieu de la partie taguée.",
 'chevire-dessous':"Au croisement : MNT 6,9 m (piste au sol) vs MNS 57,0 m (tablier) ; une lecture MNS naïve fausse l'altitude de ~50 m.",
 'rotonde-dessous':"Au croisement : MNT 7,8 m vs MNS 14,2 m.",
 'recoumene':"MNH au milieu 60,8 m (MNS 949,7 / MNT 888,9) : viaduc haut confirmé par le LiDAR.",
 'grands-goulets-tunnel':"MNT au milieu 805 m contre ~608 m et ~680 m aux portails : ~160 m de relief au-dessus de la droite entre portails.",
 'rousset':"MNT au milieu ~1435 m contre ~1250 m aux portails.",
 'croix-rousse':"MNT au milieu ~251 m contre 171/181 m aux portails.",
 're-pont':"RGE ALTI renvoie -99999 au milieu (mer hors emprise) ; LiDAR HD MNT 1,0 / MNS 39,8 m.",
 'combe-laval':"Portail B : MNT 907 m vs MNS 993 m (falaise en surplomb) ; MNS inutilisable aux portails.",
 'bourne-arbois':"Écarts RGE ALTI / LiDAR MNT de 20 à 55 m aux trois points (A, milieu, B) : zone de falaise, les deux MNT divergent.",
 'voie-verte-24':"MNH 16–20 m aux portails (végétation au-dessus de la tranchée) : utiliser le MNT, pas le MNS.",
}
ABROAD={'liege-belle-liegeoise','venise-liberta','donostia-zurriola','krk'}
MAPT={'re-pont':'z13=200','venise-liberta':'z13=200','donostia-zurriola':'z13=200','liege-belle-liegeoise':'z13=200','stn-pont':'z13=200','krk':'z13=404 (2 tuiles)'}
L=["# Corpus d'ouvrages de référence — banc qualité osm-route-details",
"# Généré le 2026-10-30 à partir de requêtes Overpass (overpass-api.de, base OSM du 2026-10-30)",
"# et de l'API altimétrie IGN Géoplateforme (data.geopf.fr). Voir rapport pour la liste des requêtes.",
"# Coordonnées WGS84 (lat, lon). Longueurs = somme géodésique (haversine) des ways de `osm.chain`.",
"# `milieu` = point à mi-longueur curviligne de la chaîne (pour une chaîne discontinue, trous inclus).",
"# Altitudes en mètres, référentiel vertical renvoyé par l'API IGN (NGF-IGN69 en métropole).",
"# null = pas de donnée (-99999 renvoyé par l'API : hors emprise).",
"version: 1",
"date_constitution: \"2026-10-30\"",
"sources:",
"  osm: {api: \"https://overpass-api.de/api/interpreter\", licence: \"ODbL 1.0\"}",
"  ign_alti: {api: \"https://data.geopf.fr/altimetrie/1.0/calcul/alti/rest/elevation.json\", licence: \"Etalab 2.0 (Licence Ouverte)\",",
"             ressources_utilisees: [ign_rge_alti_wld, ign_lidar_hd_mnx_multi_wld],",
"             liste_ressources: \"https://data.geopf.fr/altimetrie/resources (GET, 200) ; /altimetrie/1.0/calcul/resources renvoie 405\"}",
"methode_reference_defaut: >-",
"  IGN altimétrie REST elevation.json, une requête par lot de ≤30 points séparés par |, ≤ 2 req/s.",
"  (1) resource=ign_rge_alti_wld&zonly=true → `rge_alti_mnt` (MNT RGE ALTI, sol nu).",
"  (2) resource=ign_lidar_hd_mnx_multi_wld&zonly=false&measures=true → `lidar_mnt`, `lidar_mns`, `lidar_mnh`",
"  (LiDAR HD 50 cm sans interpolation, mesures « France Métropolitaine »). Points : culées/portails A et B",
"  = extrémités de la chaîne OSM, milieu = mi-longueur. Le MNS est la surface la plus haute vue par le",
"  LiDAR (tablier, mais aussi garde-corps, véhicules, végétation, falaise en surplomb) : c'est un indice",
"  de hauteur, pas une vérité terrain de la chaussée. Pour un tunnel, le MNT au milieu est le relief",
"  au-dessus, pas la chaussée.",
"ouvrages:"]
for o in G:
    i=o['id']; L.append(f"  - id: {i}")
    L.append(f"    nom: {J(o['name'])}")
    L.append(f"    type: {J(o['type'])}")
    L.append(f"    region: {J(o['region'])}")
    L.append(f"    cas_teste: {J(o['cas'])}")
    L.append(f"    osm:")
    L.append(f"      chain: {J(o['chain'])}")
    L.append(f"      ways_associees: {J(o['extra'])}")
    L.append(f"      verifie: \"Overpass, tags+géométrie lus le 2026-10-30\"")
    L.append(f"      tags:")
    for w,t in o['tags'].items():
        L.append(f"        {w}: {J(t)}")
    L.append(f"    longueur_m: {o['length_m']}")
    L.append(f"    milieu: {{lat: {o['mid'][0]:.6f}, lon: {o['mid'][1]:.6f}}}")
    L.append(f"    points: {{" + ", ".join(f"{k}: [{v[0]}, {v[1]}]" for k,v in o['pts'].items()) + "}")
    L.append(f"    reference:")
    if i in ABROAD:
        if i=='krk':
            L.append(f"      methode: \"Aucune altitude IGN (hors France, non interrogé). Test de provenance Mapterhorn : HEAD tiles.mapterhorn.com/{{z}}/{{x}}/{{y}}.webp\"")
            L.append(f"      valeurs: null")
        else:
            L.append(f"      methode: \"methode_reference_defaut ; l'API IGN renvoie -99999 (hors emprise) sur tous les points → pas de référence IGN. Test de provenance Mapterhorn : HEAD tiles.mapterhorn.com/{{z}}/{{x}}/{{y}}.webp\"")
            L.append(f"      valeurs: null")
        L.append(f"      mapterhorn_z13_milieu: {J(MAPT[i])}")
    else:
        L.append(f"      methode: methode_reference_defaut")
        L.append(f"      valeurs:")
        for k in o['pts']:
            v=ign[(i,k)]
            L.append(f"        {k}: {J(v)}")
        if i in MAPT: L.append(f"      mapterhorn_z13_milieu: {J(MAPT[i])}")
    L.append(f"      date_mesure: \"2026-10-30\"")
    L.append(f"    renvoi_vcyclist: {J(o['vc'])}")
    L.append(f"    licence: \"ODbL (OSM) / Etalab 2.0 (IGN)\"")
    L.append(f"    notes: {J(o['notes'])}")
    if i in OBS: L.append(f"    observations_reference: {J(OBS[i])}")
open('ouvrages.yaml','w').write("\n".join(L)+"\n")
print(len(G))
