"""Fixed-depth logical immutable binary radix path bound; no backend allocation."""
import validate as v
TAGS={k:k.upper().encode().ljust(8,b'_')[:8] for k in v.WORK['key_components']}
def encode(kind,components):
 bounds=v.WORK['key_components'][kind];v.require(len(components)==len(bounds),'INDEX_ARITY');raw=TAGS[kind]+bytes([len(bounds)])
 for x,bound in zip(components,bounds):
  b=x.encode('utf8');v.require(len(b)<=bound,'INDEX_COMPONENT');raw+=len(b).to_bytes(2,'big')+b
 v.require(len(raw)<=v.WORK['maximum_key_bytes'],'INDEX_K');return raw

def path(kind,components):
 b=encode(kind,components);bits=''.join(format(x,'08b') for x in b)+'1';return bits+'0'*(v.WORK['binary_radix_depth']-len(bits))
def changed_pages(kind,components):return len(path(kind,components))+1

def self_test():
 for kind,bounds in v.WORK['key_components'].items():
  components=['x'*n for n in bounds];assert changed_pages(kind,components)==v.WORK['pages_per_index_update']
 longest=['x'*n for n in v.WORK['key_components']['revision']];assert len(encode('revision',longest))==1079
 a=['x'*n for n in v.WORK['key_components']['case']];b=list(a);b[-1]=b[-1][:-1]+'y';assert path('case',a)!=path('case',b)
 # Tuple framing preserves boundaries even where concatenation would collide.
 a=['a','bc','d','e'];b=['ab','c','d','e'];assert encode('delivery',a)!=encode('delivery',b)
 return dict(K=1079,L=8633,P=128,pages_per_update=8634)
if __name__=='__main__':print(self_test())
