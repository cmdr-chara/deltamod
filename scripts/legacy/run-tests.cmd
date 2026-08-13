@echo off
pushd "%~dp0\..\.."
npm test
popd
set /p="Press Enter."
