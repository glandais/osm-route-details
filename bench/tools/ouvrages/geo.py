import json,math,sys
def hav(a,b):
    R=6371008.8
    la1,lo1,la2,lo2=map(math.radians,(a[0],a[1],b[0],b[1]))
    h=math.sin((la2-la1)/2)**2+math.cos(la1)*math.cos(la2)*math.sin((lo2-lo1)/2)**2
    return 2*R*math.asin(math.sqrt(h))
def info(e):
    g=[(p['lat'],p['lon']) for p in e['geometry']]
    seg=[hav(g[i],g[i+1]) for i in range(len(g)-1)]
    L=sum(seg); half=L/2; acc=0; mid=g[0]
    for i,s in enumerate(seg):
        if acc+s>=half:
            t=(half-acc)/s if s else 0
            mid=(g[i][0]+t*(g[i+1][0]-g[i][0]), g[i][1]+t*(g[i+1][1]-g[i][1])); break
        acc+=s
    return dict(id=e['id'],L=round(L),a=g[0],b=g[-1],mid=mid,tags=e.get('tags',{}))
KEYS=['name','highway','bridge','tunnel','layer','bridge:structure','man_made','covered','bicycle','cycleway','oneway','ref','access','motor_vehicle','construction','ele','embankment','cutting']
if __name__=='__main__':
    d=json.load(open(sys.argv[1]))
    for e in sorted(d['elements'],key=lambda e:str(e.get('tags',{}).get('name'))):
        if e['type']!='way' or 'geometry' not in e: continue
        i=info(e); t=i['tags']
        print(i['id'],i['L'],'m mid=%.6f,%.6f'%i['mid'],'A=%.6f,%.6f'%i['a'],'B=%.6f,%.6f'%i['b'],{k:t[k] for k in KEYS if k in t})
