#!/usr/bin/env python3
"""Temporary credentials stay in memory; never uses the live workspace."""
import json, os, pathlib, secrets, shutil, ssl, subprocess, tempfile, time, urllib.request, urllib.error, sqlite3
import argparse,socket
parser=argparse.ArgumentParser(description="Exercise A11y account APIs against an isolated local workspace")
parser.add_argument('--binary',required=True,type=pathlib.Path)
args=parser.parse_args()
folder=pathlib.Path(tempfile.mkdtemp(prefix='a11y-account-acceptance-'));os.chmod(folder,0o700)
binary=args.binary.resolve()
with socket.socket() as sock:
 sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
password=secrets.token_urlsafe(24)
subprocess.run([str(binary),'--bootstrap-admin',str(folder),'admin'],input=password+'\n',text=True,check=True,stdout=subprocess.DEVNULL)
log=open(folder/'server.log','w')
proc=subprocess.Popen([str(binary),str(folder),'127.0.0.1:'+str(port)],stdout=log,stderr=log)
ctx=None
def call(path,body=None,token='',expected=200):
 req=urllib.request.Request('http://127.0.0.1:'+str(port)+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+token})
 try:
  with urllib.request.urlopen(req,context=ctx,timeout=8) as r:status=r.status;result=json.load(r)
 except urllib.error.HTTPError as e:status=e.code;result=json.load(e)
 assert status==expected, (path,status,expected,result.get('error'))
 return result
try:
 for _ in range(60):
  try:call('/v1/files',expected=401);break
  except (OSError,urllib.error.URLError):time.sleep(.2)
 else:raise RuntimeError('Server did not start')
 call('/v1/files',token='old-workspace-key',expected=401)
 temp=call('/v1/auth/login',{'username':'admin','password':password})
 call('/v1/files',token=temp['token'],expected=401)
 call('/v1/admin/users',token=temp['token'],expected=403)
 permanent=secrets.token_urlsafe(24)
 a=call('/v1/auth/password',{'current_password':password,'password':permanent},temp['token'])
 call('/v1/auth/me',token=temp['token'],expected=403)
 users=call('/v1/admin/users',token=a['token']);assert len(users)==1
 vp=secrets.token_urlsafe(24)
 viewer=call('/v1/admin/users',{'username':'viewer','password':vp,'admin':False,'access':'viewer'},a['token'])
 vt=call('/v1/auth/login',{'username':'viewer','password':vp})
 newvp=secrets.token_urlsafe(24)
 v=call('/v1/auth/password',{'current_password':vp,'password':newvp},vt['token'])
 call('/v1/admin/users',token=v['token'],expected=403)
 call('/v1/admin/users',{'username':'rogue','password':permanent,'admin':True,'access':'editor'},v['token'],expected=403)
 doc=json.dumps({'name':'Acceptance only','nodes':{},'pages':[],'styles':{},'images':{},'next_id':0})
 call('/v1/files',{'project':'Acceptance','document':doc},v['token'],expected=400)
 file=call('/v1/files',{'project':'Acceptance','document':doc},a['token'],expected=201)
 assert len(call('/v1/files',token=v['token'])['files'])==1
 call('/v1/admin/grants',{'user_id':viewer['id'],'kind':'file','resource':file['id'],'role':'none'},a['token'])
 call('/v1/files',token=v['token'],expected=401)
 v=call('/v1/auth/login',{'username':'viewer','password':newvp})
 assert not call('/v1/files',token=v['token'])['files']
 call('/v1/files/'+file['id']+'/open',{},v['token'],expected=400)
 ep=secrets.token_urlsafe(24)
 editor=call('/v1/admin/users',{'username':'editor','password':ep,'admin':False,'access':'editor'},a['token'])
 et=call('/v1/auth/login',{'username':'editor','password':ep})
 en=secrets.token_urlsafe(24)
 e=call('/v1/auth/password',{'current_password':ep,'password':en},et['token'])
 call('/v1/admin/grants',{'user_id':editor['id'],'kind':'project','resource':'Shared project','role':'none'},a['token'])
 e=call('/v1/auth/login',{'username':'editor','password':en})
 call('/v1/files',{'project':'  ','document':doc},e['token'],expected=400)
 call('/v1/admin/update',{'id':a['user']['id'],'active':False,'admin':False,'access':'none'},a['token'],expected=403)
 call('/v1/admin/reset',{'id':viewer['id'],'password':secrets.token_urlsafe(24)},a['token'])
 call('/v1/auth/me',token=v['token'],expected=403)
 call('/v1/admin/update',{'id':viewer['id'],'active':False,'admin':False,'access':'viewer'},a['token'])
 call('/v1/auth/login',{'username':'viewer','password':newvp},expected=403)
 db=sqlite3.connect(folder/'accounts.sqlite')
 hashes=[r[0] for r in db.execute('SELECT password FROM users')];assert all(h.startswith('$argon2id$') for h in hashes)
 session_hashes=[r[0] for r in db.execute('SELECT digest FROM sessions')];assert a['token'] not in session_hashes;db.close()
 call('/v1/auth/logout',{},a['token']);call('/v1/auth/me',token=a['token'],expected=403)
 print('PASS: account login, forced password change, admin authorization, file filtering, viewer publish rejection, grant revocation, reset/disable, last-admin protection, hashed credentials and logout.')
finally:
 proc.terminate()
 try:proc.wait(timeout=10)
 except subprocess.TimeoutExpired:proc.kill();proc.wait()
 log.close();shutil.rmtree(folder)
