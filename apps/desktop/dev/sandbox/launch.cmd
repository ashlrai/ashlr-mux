@echo off
rem Double-click entry point for the cmux-desktop Windows Sandbox harness.
rem Builds the app, stages runtime deps, and launches it in a SAC-free sandbox.
rem Pass -SkipBuild to relaunch the existing build without rebuilding.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-sandbox.ps1" %*
