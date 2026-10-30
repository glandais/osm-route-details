import json,sys
from geo import KEYS
def segs(e): g=[(p['lon'],p['lat']) for p in e['geometry']]; return list(zip(g,g[1:]))
def inter(s,t):
    (x1,y1),(x2,y2)=s;(x3,y3),(x4,y4)=t
    d=(x2-x1)*(y4-y3)-(y2-y1)*(x4-x3)
    if d==0: return None
    u=((x3-x1)*(y4-y3)-(y3-y1)*(x4-x3))/d; v=((x3-x1)*(y2-y1)-(y3-y1)*(x2-x1))/d
    e=1e-6
    if e<u<1-e and e<v<1-e: return (y1+u*(y2-y1), x1+u*(x2-x1))
def run(f,ids,f2=None):
    d=json.load(open(f)); W={e['id']:e for e in d['elements'] if e['type']=='way' and 'geometry' in e}
    if f2:
        for e in json.load(open(f2))['elements']:
            if 'geometry' in e: W.setdefault(e['id'],e)
    for b in ids:
        if b not in W: print('missing',b); continue
        B=W[b]; print('==',b,{k:B['tags'][k] for k in KEYS if k in B['tags']})
        for w,E in W.items():
            if w==b: continue
            for s in segs(B):
                p=None
                for t in segs(E):
                    p=inter(s,t)
                    if p: break
                if p:
                    print('   x',w,'%.6f,%.6f'%p,{k:E['tags'][k] for k in KEYS+['railway','waterway','service'] if k in E['tags']}); break
if __name__=='__main__': run(sys.argv[1],[int(x) for x in sys.argv[2].split(',')], sys.argv[3] if len(sys.argv)>3 else None)
