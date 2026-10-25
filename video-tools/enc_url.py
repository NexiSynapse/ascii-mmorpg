import sys
from urllib.parse import quote
raw = sys.argv[1]
base, _, query = raw.partition("?")
q = "&".join(f"{k}={quote(v, safe='')}" for kv in query.split("&") for k, _, v in [kv.partition("=")])
print(base[:base.rfind("/")+1] + quote(base[base.rfind("/")+1:], safe="") + "?" + q)
