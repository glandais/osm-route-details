import json,glob,math,time,subprocess,urllib.parse,sys
sys.path.insert(0,'.')
from geo import hav,KEYS
from cross import inter,segs
from corpus import C
W={}
for f in sorted(glob.glob('raw/q*.json')):
    try: d=json.load(open(f))
    except Exception: continue
    for e in d.get('elements',[]):
        if e.get('type')=='way' and 'geometry' in e: W[e['id']]=e
def g(w): return [(p['lat'],p['lon']) for p in W[w]['geometry']]
def plen(p): return sum(hav(p[i],p[i+1]) for i in range(len(p)-1))
def at(p,frac):
    L=plen(p); tgt=L*frac; acc=0
    for i in range(len(p)-1):
        s=hav(p[i],p[i+1])
        if acc+s>=tgt:
            t=(tgt-acc)/s if s else 0
            return (p[i][0]+t*(p[i+1][0]-p[i][0]), p[i][1]+t*(p[i+1][1]-p[i][1]))
        acc+=s
    return p[-1]
out=[]
for c in C:
    miss=[w for w in c['chain']+c['extra'] if w not in W]
    if miss: print('MISSING',c['id'],miss)
    poly=[]; L=0
    for w in c['chain']:
        p=g(w); L+=plen(p)
        if poly:
            # orienter
            if hav(poly[-1],p[-1])<hav(poly[-1],p[0]): p=p[::-1]
            if len(poly)==1 or hav(poly[-1],poly[0])==0: pass
        else:
            if len(c['chain'])>1:
                q=g(c['chain'][1])
                if min(hav(p[0],q[0]),hav(p[0],q[-1]))<min(hav(p[-1],q[0]),hav(p[-1],q[-1])): p=p[::-1]
        poly+=p
    pts={'A':poly[0],'milieu':at(poly,0.5),'B':poly[-1]}
    if c.get('cross'):
        b,u=c['cross']; P=None
        for s in segs(W[b]):
            for t in segs(W[u]):
                r=inter(s,t)
                if r: P=r;break
            if P: break
        pts={'croisement':P, 'A_voie_dessous':poly[0],'B_voie_dessous':poly[-1]}
    if c.get('start_mid'):
        pts={'depart_sur_tablier':at(poly,0.5),'B':poly[-1]}
    for lab,w,fr in c['pts_from']:
        pts[lab]=at(g(w),fr)
    tags={}
    for w in c['chain']+c['extra']:
        if w in W: tags[w]={k:v for k,v in W[w]['tags'].items() if k in KEYS+['tunnel','covered','name']}
    out.append(dict(c, length_m=round(L), mid=at(poly,0.5), pts={k:(round(v[0],6),round(v[1],6)) for k,v in pts.items()}, tags=tags))
json.dump(out,open('corpus_geom.json','w'),ensure_ascii=False,indent=1)
for o in out: print(o['id'],o['length_m'],o['pts'])
