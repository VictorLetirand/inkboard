@echo off
rem Ouvre le tableau blanc dans une fenetre d'application (Edge ou Chrome), sans barre d'adresse.
set "URL=file:///%~dp0index.html"
set "URL=%URL:\=/%"
set "URL=%URL: =%20%"
if exist "%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe" ( start "" "%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe" --app="%URL%" & exit /b )
if exist "%ProgramFiles%\Microsoft\Edge\Application\msedge.exe" ( start "" "%ProgramFiles%\Microsoft\Edge\Application\msedge.exe" --app="%URL%" & exit /b )
if exist "%ProgramFiles%\Google\Chrome\Application\chrome.exe" ( start "" "%ProgramFiles%\Google\Chrome\Application\chrome.exe" --app="%URL%" & exit /b )
start "" "%~dp0index.html"
