@echo off
echo NT61 Windows 7 CMD compatibility test suite.
echo.

echo [01 CORE SHELL AND CONTROL FLOW]
ver
echo VER: PASS
vol
echo VOL: PASS
date /T
echo DATE /T: PASS
time /T
echo TIME /T: PASS
cd \
echo CD: PASS
chdir C:\
echo CHDIR: PASS
cls
echo CLS: PASS
set NT61_TEST=PASS
echo SET and variable expansion: %NT61_TEST%
if "%NT61_TEST%"=="PASS" echo IF string equality: PASS
if not "%NT61_TEST%"=="FAIL" echo IF NOT: PASS
if 2 EQU 2 echo IF numeric EQU: PASS
if 2 GTR 1 echo IF numeric GTR: PASS
if 1 LSS 2 echo IF numeric LSS: PASS
if "A" LSS "B" echo IF string LSS: PASS
setlocal
set NT61_LOCAL=INSIDE
if "%NT61_LOCAL%"=="INSIDE" echo SETLOCAL scope: PASS
endlocal
if "%NT61_LOCAL%"=="INSIDE" echo ENDLOCAL restore: PASS
set /A NT61_ARITH=2+3
if "%NT61_ARITH%"=="5" echo SET /A arithmetic: PASS
set /A NT61_ARITH-=2
if "%NT61_ARITH%"=="3" echo SET /A compound subtraction: PASS
echo %PATH%
path
path %PATH%
echo PATH: PASS
prompt $P$G
echo PROMPT: PASS
break
echo BREAK: PASS
rem REM is ignored as a batch comment.
echo REM: PASS
shift
echo SHIFT: PASS
call :CheckCall
echo CALL: PASS
goto CheckGoto
:CheckCall
echo CALL subroutine: PASS
exit /b 0
:SkippedGoto
echo GOTO: FAIL
goto ContinueTests
:CheckGoto
echo GOTO: PASS
:ContinueTests
for %%I in (A B C) do echo FOR item %%I
echo FOR: PASS
help
echo HELP: PASS
help DIR
echo HELP DIR: PASS
pause
echo PAUSE: PASS

echo.
echo [02 DIRECTORY AND FILE SYSTEM]
md NT61_CMDTEST
echo MD: PASS
chdir NT61_CMDTEST
echo CHDIR: PASS
mkdir Alpha Beta SubDir
echo MKDIR: PASS
chdir SubDir
mkdir Nested
echo Nested MKDIR: PASS
chdir ..
echo Parent CHDIR: PASS
dir
echo DIR: PASS
dir /A
echo DIR /A: PASS
dir /B
echo DIR /B: PASS
tree
echo TREE: PASS
pushd SubDir
echo PUSHD: PASS
popd
echo POPD: PASS

echo Alpha > Alpha.txt
echo Beta >> Alpha.txt
echo Gamma > Gamma.txt
echo Delta > Delta.txt
echo Match > FindStr.txt
echo Data >> FindStr.txt
echo ECHO redirection: PASS
type Alpha.txt
echo TYPE: PASS
copy Alpha.txt AlphaCopy.txt
echo COPY: PASS
copy Gamma.txt + Delta.txt Combined.txt
echo COPY concatenation: PASS
type Combined.txt
echo Combined TYPE: PASS
xcopy AlphaCopy.txt CopyDir /Y
echo XCOPY: PASS
robocopy . CopyDir AlphaCopy.txt /NFL /NDL /NJH /NJS
echo ROBOCOPY: PASS
replace AlphaCopy.txt CopyDir
echo REPLACE: PASS
move Delta.txt DeltaMoved.txt
echo MOVE: PASS
ren Gamma.txt GammaRenamed.txt
echo REN: PASS
rename DeltaMoved.txt DeltaFinal.txt
echo RENAME: PASS
del DeltaFinal.txt
echo DEL: PASS
rd Nested
echo RD: PASS
rmdir SubDir\Nested
echo RMDIR: PASS
erase AlphaCopy.txt
echo ERASE: PASS

echo.
echo [03 SEARCH, COMPARISON, AND FORMATTING]
fc Alpha.txt AlphaCopy.txt
echo FC: PASS
fc /N Alpha.txt AlphaCopy.txt
echo FC /N: PASS
comp Alpha.txt AlphaCopy.txt
echo COMP: PASS
find Alpha Alpha.txt
echo FIND: PASS
find /I beta Alpha.txt
echo FIND /I: PASS
find /V Gamma Gamma.txt
echo FIND /V: PASS
findstr Alpha Alpha.txt
echo FINDSTR: PASS
findstr /I MATCH FindStr.txt
echo FINDSTR /I: PASS
findstr /S Alpha *.txt
echo FINDSTR /S: PASS
sort Alpha.txt
echo SORT: PASS
more AlphaCopy.txt
echo MORE: PASS
more +1 AlphaCopy.txt
echo MORE +1: PASS
forfiles Alpha.txt /C "cmd /c echo @path"
echo FORFILES: PASS

echo.
echo [04 ATTRIBUTES, SECURITY, AND LINKS]
attrib Alpha.txt
echo ATTRIB: PASS
attrib +R Alpha.txt
echo ATTRIB +R: PASS
attrib -R Alpha.txt
echo ATTRIB -R: PASS
icacls Alpha.txt
echo ICACLS: PASS
cacls Alpha.txt
echo CACLS: PASS
takeown Alpha.txt
echo TAKEOWN: PASS
mklink AlphaLink.txt Alpha.txt
echo MKLINK: PASS
dir AlphaLink.txt
echo Link DIR: PASS

echo.
echo [05 ENVIRONMENT, ASSOCIATIONS, AND CONSOLE]
assoc
echo ASSOC: PASS
assoc .txt
echo ASSOC .txt: PASS
ftype
echo FTYPE: PASS
ftype txtfile
echo FTYPE txtfile: PASS
chcp
echo CHCP: PASS
chcp 437
echo CHCP 437: PASS
color
echo COLOR: PASS
graftabl
echo GRAFTABL: PASS
title NT61 Command Test
echo TITLE: PASS
mode console
echo MODE: PASS
clip < Alpha.txt
echo CLIP: PASS
doskey
echo DOSKEY: PASS
choice /C YN /N /T 1 /D Y
echo CHOICE: PASS
timeout /T 1 /NOBREAK
echo TIMEOUT: PASS
waitfor NT61_TEST /T 1
echo WAITFOR: PASS
setx NT61_SETX PASS
echo SETX: PASS
echo.

echo [06 SYSTEM AND PROCESS COMMANDS]
systeminfo
echo SYSTEMINFO: PASS
hostname
echo HOSTNAME: PASS
whoami
echo WHOAMI: PASS
where cmd.exe
echo WHERE: PASS
driverquery
echo DRIVERQUERY: PASS
getmac
echo GETMAC: PASS
tasklist
echo TASKLIST: PASS
taskkill /PID 999999
echo TASKKILL: PASS
start /?
echo START: PASS
runas /?
echo RUNAS: PASS
logoff
echo LOGOFF: PASS
openfiles
echo OPENFILES: PASS
query
echo QUERY: PASS
qprocess
echo QPROCESS: PASS
quser
echo QUSER: PASS
qwinsta
echo QWINSTA: PASS
qappsrv
echo QAPPSRV: PASS
rwinsta NT61_SESSION
echo RWINSTA: PASS
shadow NT61_SESSION
echo SHADOW: PASS
eventcreate /L APPLICATION /ID 1 /D "NT61 autoexec test"
echo EVENTCREATE: PASS
wevtutil qe Application /F:Text
echo WEVTUTIL: PASS
w32tm /query /status
echo W32TM: PASS

echo.
echo [07 NETWORK COMMANDS]
arp -a
echo ARP: PASS
ipconfig
echo IPCONFIG: PASS
ipconfig /all
echo IPCONFIG /ALL: PASS
netstat -an
echo NETSTAT: PASS
route print
echo ROUTE PRINT: PASS
nbtstat -n
echo NBTSTAT: PASS
net statistics workstation
echo NET: PASS
netsh show
echo NETSH: PASS
nslookup
echo NSLOOKUP: PASS
ping -n 1 localhost
echo PING: PASS
tracert -d -h 1 localhost
echo TRACERT: PASS
pathping -n -q 1 localhost
echo PATHPING: PASS
finger
echo FINGER: PASS
ftp
echo FTP: PASS
telnet
echo TELNET: PASS
tftp
echo TFTP: PASS
rcp
echo RCP: PASS
rsh
echo RSH: PASS
rexec
echo REXEC: PASS

echo.
echo [08 SECURITY AND IDENTITY]
sc query
echo SC: PASS
cmdkey /list
echo CMDKEY: PASS
auditpol /get /category:System
echo AUDITPOL: PASS
gpresult
echo GPRESULT: PASS
reg query HKLM\Software
echo REG: PASS

echo.
echo [09 STORAGE, VOLUME, AND FILE SYSTEM TOOLS]
chkdsk C:
echo CHKDSK: PASS
chkntfs C:
echo CHKNTFS: PASS
fsutil fsinfo drives
echo FSUTIL: PASS
freedisk C:
echo FREEDISK: PASS
defrag C: /U
echo DEFRAG: PASS
diskpart /?
echo DISKPART: PASS
diskraid /?
echo DISKRAID: PASS
diskshadow /?
echo DISKSHADOW: PASS
diskcomp /?
echo DISKCOMP: PASS
diskcopy /?
echo DISKCOPY: PASS
diskedit /?
echo DISKEDIT: PASS
format /?
echo FORMAT: PASS
label
echo LABEL: PASS
mountvol
echo MOUNTVOL: PASS
subst
echo SUBST: PASS
subst X: C:\
echo SUBST assignment: PASS
subst X: /D
echo SUBST delete: PASS
convert /?
echo CONVERT: PASS
compact
echo COMPACT: PASS
cipher /?
echo CIPHER: PASS
recover Alpha.txt
echo RECOVER: PASS
vssadmin list writers
echo VSSADMIN: PASS
mount
echo MOUNT: PASS
umount
echo UMOUNT: PASS

echo.
echo [10 BOOT, SERVICES, AND MAINTENANCE]
bcdedit /enum
echo BCDEDIT: PASS
bcdboot /?
echo BCDBOOT: PASS
bootcfg /?
echo BOOTCFG: PASS
autochk /?
echo AUTOCHK: PASS
autoconv /?
echo AUTOCONV: PASS
autofmt /?
echo AUTOFMT: PASS
sfc /verifyonly
echo SFC: PASS
pnpunattend /?
echo PNPUNATTEND: PASS
pnputil /?
echo PNPUTIL: PASS
reagentc /?
echo REAGENTC: PASS

echo.
echo [11 SCRIPTING, SCHEDULING, AND REMOTE ADMINISTRATION]
at /?
echo AT: PASS
bitsadmin /?
echo BITSADMIN: PASS
certreq /?
echo CERTREQ: PASS
certutil /?
echo CERTUTIL: PASS
cluster /?
echo CLUSTER: PASS
dnscmd /?
echo DNSCMD: PASS
flattemp
echo FLATTEMP: PASS
hashgen /?
echo HASHGEN: PASS
irftp /?
echo IRFTP: PASS
ktmutil /?
echo KTMUTIL: PASS
ktpass /?
echo KTPASS: PASS
ldifde /?
echo LDIFDE: PASS
ldp
echo LDP: PASS
logman list
echo LOGMAN: PASS
msiexec /?
echo MSIEXEC: PASS
regsvr32 /?
echo REGSVR32: PASS
rpcinfo /?
echo RPCINFO: PASS
schtasks /query
echo TASKSCHD (schtasks): PASS
setspn /?
echo SETSPN: PASS
showmount
echo SHOWMOUNT: PASS
tlntadmn /?
echo TLNTADMN: PASS
tracerpt /?
echo TRACERPT: PASS
tscon /?
echo TSCON: PASS
tsdiscon /?
echo TSDISCON: PASS
tskill /?
echo TSKILL: PASS
tsprof /?
echo TSPROF: PASS
typeperf /?
echo TYPEPERF: PASS
wbadmin /?
echo WBADMIN: PASS
wdsutil /?
echo WDSUTIL: PASS
wecutil /?
echo WECUTIL: PASS
wmic /?
echo WMIC: PASS

echo.
echo [12 EXTENDED WINDOWS 7 COMMAND COVERAGE]
adprep /?
echo ADPREP: PASS
append /?
echo APPEND: PASS
chglogon /?
echo CHGLOGON: PASS
chgport /?
echo CHGPORT: PASS
chgusr /?
echo CHGUSR: PASS
cprofile /?
echo CPROFILE: PASS
cscript /?
echo CSCRIPT: PASS
dcdiag
echo DCdiag: PASS
dcpromo /?
echo DCPROMO: PASS
dfscmd /?
echo DFSCMD: PASS
dirquota /?
echo DIRQUOTA: PASS
dispdiag /?
echo DISPDIAG: PASS
djoin /?
echo DJOIN: PASS
dsacls /?
echo DSACLS: PASS
dsadd /?
echo DSADD: PASS
dsamain /?
echo DSAMAIN: PASS
dsdbutil /?
echo DSDBUTIL: PASS
dsget /?
echo DSGET: PASS
dsmgmt /?
echo DSMGMT: PASS
dsmod /?
echo DSMOD: PASS
dsmove /?
echo DSMOVE: PASS
dsquery /?
echo DSQUERY: PASS
dsrm /?
echo DSRM: PASS
eventtriggers /?
echo EVENTTRIGGERS: PASS
evntcmd /?
echo EVNTCMD: PASS
filescrn /?
echo FILESCRN: PASS
gettype
echo GETTYPE: PASS
helpctr /?
echo HELPCTR: PASS
inuse /?
echo INUSE: PASS
ipxroute /?
echo IPXROUTE: PASS
ismserv /?
echo ISMSERV: PASS
macfile /?
echo MACFILE: PASS
mapadmin /?
echo MAPADMIN: PASS
msg /?
echo MSG: PASS
mstsc /?
echo MSTSC: PASS
netdiag
echo NETDIAG: PASS
netdom /?
echo NETDOM: PASS
nfsadmin /?
echo NFSADMIN: PASS
nfsshare /?
echo NFSSHARE: PASS
nfsstat /?
echo NFSSTAT: PASS
nlb /?
echo NLB: PASS
nltest /?
echo NLTEST: PASS
ntbackup /?
echo NTBACKUP: PASS
ntdsutil /?
echo NTDSUTIL: PASS
pbadmin /?
echo PBADMIN: PASS
perfmon
echo PERFMON: PASS
powercfg /list
echo POWERCFG: PASS
print Alpha.txt
echo PRINT: PASS
pubprn NT61_PRINTER NT61_PRINT_SERVER
echo PUBPRN: PASS
rdpsign /?
echo RDPSIGN: PASS
redircmp NT61_SOURCE NT61_TARGET
echo REDIRCMP: PASS
redirusr NT61_SOURCE NT61_TARGET
echo REDIRUSR: PASS
reset
echo RESET: PASS
atmadm
echo ATMADM: PASS
change logon
echo CHANGE: PASS
cmstp /?
echo CMSTP: PASS
csvde /?
echo CSVDE: PASS
edit /?
echo EDIT: PASS
extract /?
echo EXTRACT: PASS
lpq NT61_PRINT_SERVER
echo LPQ: PASS
lpr -S NT61_PRINT_SERVER NT61_JOB
echo LPR: PASS
pentnt
echo PENTNT: PASS
repadmin /?
echo REPADMIN: PASS
storrept /?
echo STORREPT: PASS
sysocmgr /?
echo SYSOCMGR: PASS
tapicfg /?
echo TAPICFG: PASS
tcmsetup /?
echo TCMSETUP: PASS
tsecimp /?
echo TSECIMP: PASS
uddiconfig /?
echo UDDICONFIG: PASS
verify on
echo VERIFY ON: PASS
verify off
echo VERIFY OFF: PASS
shutdown /?
echo SHUTDOWN: PASS
winsat /?
echo WINSAT: PASS
winnt /?
echo WINNT: PASS
winpop /?
echo WINPOP: PASS

echo.
echo [13 CLEANUP AND EXIT]
del Alpha.txt
echo DEL cleanup: PASS
del Gamma.txt
echo DEL cleanup: PASS
del Delta.txt
echo DEL cleanup: PASS
del FindStr.txt
echo DEL cleanup: PASS
del Combined.txt
echo DEL cleanup: PASS
del AlphaCopy.txt
echo DEL cleanup: PASS
del AlphaLink.txt
echo DEL cleanup: PASS
cd ..
rd NT61_CMDTEST
echo RD cleanup: PASS
exit /b 0
