@echo off
chcp 65001 >nul

echo [1/4] 开始构建前端 Vue3 项目...
cd frontend
call npm run build
if %errorlevel% neq 0 (
    echo [错误] 前端构建失败，请检查前端依赖或语法。
    cd ..
    exit /b %errorlevel%
)
cd ..

echo.
echo [2/4] 同步前端构建产物到 backend/dist...
if exist backend\dist rmdir /s /q backend\dist
xcopy /s /e /y frontend\dist backend\dist >nul

echo.
echo [3/4] 设置 Linux amd64 交叉编译环境变量...
setlocal
set GOOS=linux
set GOARCH=amd64

echo.
echo [4/4] 开始编译 Go 后端二进制...
cd backend
go build -ldflags="-s -w" -o ..\fluxor
set ERR=%errorlevel%
cd ..
if %ERR% neq 0 (
    echo [错误] Go 后端二进制编译失败。
    endlocal
    exit /b %ERR%
)
endlocal

echo.
echo [成功] Linux amd64 二进制文件 "fluxor" 编译完成。
pause