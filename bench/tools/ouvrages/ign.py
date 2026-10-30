import json,time,urllib.request,urllib.parse
UA="osm-route-details-bench/0.1 (usage personnel)"
B="https://data.geopf.fr/altimetrie/1.0/calcul/alti/rest/elevation.json"
G=json.load(open('corpus_geom.json'))
P=[]
for o in G:
    for k,(la,lo) in o['pts'].items(): P.append((o['id'],k,la,lo))
def call(res,chunk,extra):
    q="lon="+"|".join("%.6f"%p[3] for p in chunk)+"&lat="+"|".join("%.6f"%p[2] for p in chunk)+"&resource="+res+extra
    req=urllib.request.Request(B+"?"+q,headers={"User-Agent":UA})
    return json.load(urllib.request.urlopen(req,timeout=60))
R={'rge':[], 'lidar':[]}; log=[]
for i in range(0,len(P),30):
    ch=P[i:i+30]
    a=call("ign_rge_alti_wld",ch,"&zonly=true"); time.sleep(0.6)
    b=call("ign_lidar_hd_mnx_multi_wld",ch,"&zonly=false&measures=true"); time.sleep(0.6)
    R['rge']+=a['elevations']; R['lidar']+=b['elevations']; log.append(len(ch))
json.dump(dict(points=P,**R),open('raw/ign_results.json','w'),ensure_ascii=False)
print(len(P),log, time.strftime('%Y-%m-%dT%H:%M:%S%z'))
