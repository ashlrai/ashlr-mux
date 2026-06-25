[CmdletBinding()]
param(
    [string]$SourcePng = "",
    [string]$OutputIco = ""
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $SourcePng) {
    $SourcePng = Join-Path $RepoRoot "Assets.xcassets\AppIcon.appiconset\256.png"
}
if (-not $OutputIco) {
    $OutputIco = Join-Path $RepoRoot "apps\desktop\src-tauri\icons\icon.ico"
}

$outputDir = Split-Path -Parent $OutputIco
New-Item -ItemType Directory -Force -Path $outputDir | Out-Null

$bitmap = [System.Drawing.Bitmap]::new($SourcePng)
try {
    $size = 256
    $resized = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($resized)
        try {
            $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
            $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $graphics.DrawImage($bitmap, 0, 0, $size, $size)
        }
        finally {
            $graphics.Dispose()
        }

        $rect = [System.Drawing.Rectangle]::new(0, 0, $size, $size)
        $bitmapData = $resized.LockBits(
            $rect,
            [System.Drawing.Imaging.ImageLockMode]::ReadOnly,
            [System.Drawing.Imaging.PixelFormat]::Format32bppArgb
        )

        try {
            $stride = [Math]::Abs($bitmapData.Stride)
            $pixelBytes = New-Object byte[] ($stride * $size)
            [System.Runtime.InteropServices.Marshal]::Copy($bitmapData.Scan0, $pixelBytes, 0, $pixelBytes.Length)
        }
        finally {
            $resized.UnlockBits($bitmapData)
        }

        $maskRowBytes = [int]([Math]::Ceiling($size / 32.0) * 4)
        $maskBytes = New-Object byte[] ($maskRowBytes * $size)
        $xorBytes = New-Object byte[] ($size * $size * 4)

        for ($y = 0; $y -lt $size; $y++) {
            $srcOffset = ($size - 1 - $y) * $stride
            $dstOffset = $y * $size * 4
            [Array]::Copy($pixelBytes, $srcOffset, $xorBytes, $dstOffset, $size * 4)
        }

        $imageSize = 40 + $xorBytes.Length + $maskBytes.Length
        $stream = [System.IO.File]::Open($OutputIco, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write)
        try {
            $writer = New-Object System.IO.BinaryWriter($stream)
            try {
                $writer.Write([UInt16]0)
                $writer.Write([UInt16]1)
                $writer.Write([UInt16]1)

                $writer.Write([byte]0)
                $writer.Write([byte]0)
                $writer.Write([byte]0)
                $writer.Write([byte]0)
                $writer.Write([UInt16]1)
                $writer.Write([UInt16]32)
                $writer.Write([UInt32]$imageSize)
                $writer.Write([UInt32]22)

                $writer.Write([UInt32]40)
                $writer.Write([Int32]$size)
                $writer.Write([Int32]($size * 2))
                $writer.Write([UInt16]1)
                $writer.Write([UInt16]32)
                $writer.Write([UInt32]0)
                $writer.Write([UInt32]($xorBytes.Length + $maskBytes.Length))
                $writer.Write([Int32]0)
                $writer.Write([Int32]0)
                $writer.Write([UInt32]0)
                $writer.Write([UInt32]0)
                $writer.Write($xorBytes)
                $writer.Write($maskBytes)
            }
            finally {
                $writer.Dispose()
            }
        }
        finally {
            $stream.Dispose()
        }
    }
    finally {
        $resized.Dispose()
    }
}
finally {
    $bitmap.Dispose()
}

Write-Host "Generated $OutputIco"
