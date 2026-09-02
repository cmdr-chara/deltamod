#requires -Version 7.0

[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $CacheRoot,
    [Parameter(Mandatory)] [string] $DeltaruneRoot,
    [Parameter(Mandatory)] [string] $UndertaleRoot,
    [Parameter(Mandatory)] [string] $ThemeRoot,
    [string] $ProvenanceOutput,
    [string[]] $ThemeId
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing.Common

$specs = @(
    [pscustomobject]@{ Id='card-castle'; Scene='card-castle'; Name='Card Castle'; Work='DELTARUNE'; Music='mus/card_castle.ogg'; Track='Card Castle'; Description='A theme based on Card Castle from DELTARUNE.'; Color='rgb(101, 84, 192)'; Soul='#FF0000' },
    [pscustomobject]@{ Id='noelle'; Scene='noelle'; Name='Noelle'; Work='DELTARUNE'; Music='mus/noelle.ogg'; Track='Noelle'; Description='A theme based on Noelle from DELTARUNE.'; Color='rgb(42, 116, 148)'; Soul='#42FCFF' },
    [pscustomobject]@{ Id='tv-world'; Scene='tv-world'; Name='TV World'; Work='DELTARUNE'; Music='mus/tv_world.ogg'; Track='TV World'; Description='A theme based on TV World from DELTARUNE.'; Color='rgb(164, 48, 145)'; Soul='#D535D9' },
    [pscustomobject]@{ Id='the-knight'; Scene='the-knight'; Name='The Roaring Knight'; Work='DELTARUNE'; Music='mus/knight.ogg'; Track='The Roaring Knight'; Description='An animated theme based on the Roaring Knight from DELTARUNE.'; Color='rgb(87, 82, 151)'; Soul='#FF0000'; Video='the-knight.mp4' },
    [pscustomobject]@{ Id='undertale-ruins'; Scene='undertale-ruins'; Name='Ruins'; Work='UNDERTALE'; Music='mus_ruins.ogg'; Track='Ruins'; Description='A theme based on the Ruins from UNDERTALE.'; Color='rgb(121, 80, 165)'; Soul='#FF0000' },
    [pscustomobject]@{ Id='undertale-snowdin'; Scene='undertale-snowdin'; Name='Snowdin'; Work='UNDERTALE'; Music='mus_snowy.ogg'; Track='Snowy'; Description='A theme based on Snowdin from UNDERTALE.'; Color='rgb(50, 118, 158)'; Soul='#003CFF' },
    [pscustomobject]@{ Id='undertale-waterfall'; Scene='undertale-waterfall'; Name='Waterfall'; Work='UNDERTALE'; Music='mus_waterfall.ogg'; Track='Waterfall'; Description='A theme based on Waterfall from UNDERTALE.'; Color='rgb(56, 97, 199)'; Soul='#42FCFF' },
    [pscustomobject]@{ Id='undertale-void'; Scene='undertale-void'; Name='Barrier'; Work='UNDERTALE'; Music='mus_barrier.ogg'; Track='Barrier'; Description='A theme based on the Barrier from UNDERTALE.'; Color='rgb(105, 86, 132)'; Soul='#FFFF00' },
    [pscustomobject]@{ Id='undertale-hotland'; Scene='undertale-hotland'; Name='Hotland'; Work='UNDERTALE'; Music='mus_anothermedium.ogg'; Track='Another Medium'; Description='A theme based on Hotland from UNDERTALE.'; Color='rgb(178, 59, 31)'; Soul='#FCA600' },
    [pscustomobject]@{ Id='undertale-core'; Scene='undertale-core'; Name='CORE'; Work='UNDERTALE'; Music='mus_core.ogg'; Track='CORE'; Description='A theme based on the CORE from UNDERTALE.'; Color='rgb(64, 96, 202)'; Soul='#42FCFF' },
    [pscustomobject]@{ Id='undertale-true-lab'; Scene='undertale-true-lab'; Name='True Lab'; Work='UNDERTALE'; Music='mus_hereweare.ogg'; Track='Here We Are'; Description='A theme based on the True Lab from UNDERTALE.'; Color='rgb(110, 98, 130)'; Soul='#FF0000' },
    [pscustomobject]@{ Id='undertale-new-home'; Scene='undertale-new-home'; Name='New Home'; Work='UNDERTALE'; Music='mus_endarea_parta.ogg'; Track='Undertale'; Description='A theme based on New Home from UNDERTALE.'; Color='rgb(155, 106, 29)'; Soul='#FFFF00' }
)

$requestedThemeIds = @($ThemeId | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
if ($requestedThemeIds.Count -gt 0) {
    $knownThemeIds = @($specs.Id)
    $unknownThemeIds = @($requestedThemeIds | Where-Object { $_ -notin $knownThemeIds })
    if ($unknownThemeIds.Count -gt 0) {
        throw "Unknown theme id(s): $($unknownThemeIds -join ', ')"
    }
    $specs = @($specs | Where-Object { $_.Id -in $requestedThemeIds })
}

$cache = (Resolve-Path -LiteralPath $CacheRoot).Path
$deltarune = (Resolve-Path -LiteralPath $DeltaruneRoot).Path
$undertale = (Resolve-Path -LiteralPath $UndertaleRoot).Path
$themes = (Resolve-Path -LiteralPath $ThemeRoot).Path
$imageDirectory = Join-Path $themes 'img'
$musicDirectory = Join-Path $themes 'mus'
$dataDirectory = Join-Path $themes 'data'
$videoDirectory = Join-Path $themes 'video'

foreach ($directory in @($imageDirectory, $musicDirectory, $dataDirectory, $videoDirectory)) {
    if (-not (Test-Path -LiteralPath $directory -PathType Container)) {
        throw "Missing theme output directory: $directory"
    }
}

function Get-SceneSources {
    param([Parameter(Mandatory)] [string] $Scene)
    switch ($Scene) {
        'card-castle' { @('deltarune-ch1/Sprites/bg_cardcastle_outside_0.png') }
        'noelle' {
            @(
                'deltarune-ch2/Sprites/bg_dw_noelle_room_0.png',
                'deltarune-ch2/Sprites/spr_noelleb_spell_4.png'
            )
        }
        'tv-world' {
            @(
                'deltarune-ch3/Sprites/spr_dw_gameshow_bg_0.png',
                'deltarune-ch3/Sprites/spr_gameshow_screen_city_0.png',
                'deltarune-ch3/Sprites/spr_dw_gameshow_tv_frame_0.png',
                'deltarune-ch3/Sprites/spr_gameshow_screen_logo_0.png',
                'deltarune-ch3/Sprites/spr_dw_gameshow_podium_0.png',
                'deltarune-ch3/Sprites/spr_dw_gameshow_podium_1.png',
                'deltarune-ch3/Sprites/spr_dw_gameshow_podium_2.png',
                'deltarune-ch3/Sprites/spr_gameshow_crowd_a_0.png',
                'deltarune-ch3/Sprites/spr_gameshow_crowd_b_0.png',
                'deltarune-ch3/Sprites/spr_tenna_armsup_annoyed_0.png'
            )
        }
        'the-knight' {
            @(
                'deltarune-ch4/Sprites/spr_roaringknight_ball_transition_pose_4.png',
                'deltarune-ch4/Sprites/spr_titan_star_centered_0.png'
            )
        }
        'undertale-ruins' { @('selected-scenes/undertale-ruins.png') }
        'undertale-snowdin' { @('selected-scenes/undertale-snowdin.png') }
        'undertale-waterfall' { @('selected-scenes/undertale-waterfall.png') }
        'undertale-void' { @('selected-scenes/undertale-barrier.png') }
        'undertale-hotland' { @('selected-scenes/undertale-hotland.png') }
        'undertale-core' { @('selected-scenes/undertale-core.png') }
        'undertale-true-lab' { @('selected-scenes/undertale-true-lab.png') }
        'undertale-new-home' { @('selected-scenes/undertale-new-home.png') }
        default { throw "Unknown scene: $Scene" }
    }
}

function Resolve-SceneSource {
    param([Parameter(Mandatory)] [string] $Relative)
    $path = Join-Path $cache ($Relative.Replace('/', [System.IO.Path]::DirectorySeparatorChar))
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing scene source: $Relative" }
    return $path
}

function Set-SceneColor {
    param($Graphics, [string] $Color)
    $brush = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml($Color))
    try { $Graphics.FillRectangle($brush, 0, 0, 320, 180) } finally { $brush.Dispose() }
}

function Fill-SceneRectangle {
    param($Graphics, [string] $Color, [int] $X, [int] $Y, [int] $Width, [int] $Height)
    $brush = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml($Color))
    try { $Graphics.FillRectangle($brush, $X, $Y, $Width, $Height) } finally { $brush.Dispose() }
}

function Draw-SceneAsset {
    param($Graphics, [string] $Relative, [int] $X, [int] $Y, [int] $Width, [int] $Height)
    $image = [System.Drawing.Image]::FromFile((Resolve-SceneSource $Relative))
    try { $Graphics.DrawImage($image, [System.Drawing.Rectangle]::new($X, $Y, $Width, $Height)) } finally { $image.Dispose() }
}

function Draw-SceneCrop {
    param($Graphics, [string] $Relative, [int] $SourceX, [int] $SourceY, [int] $SourceWidth, [int] $SourceHeight)
    $image = [System.Drawing.Image]::FromFile((Resolve-SceneSource $Relative))
    try {
        $Graphics.DrawImage($image, [System.Drawing.Rectangle]::new(0, 0, 320, 180), $SourceX, $SourceY, $SourceWidth, $SourceHeight, [System.Drawing.GraphicsUnit]::Pixel)
    } finally { $image.Dispose() }
}

function Tile-SceneAsset {
    param($Graphics, [string] $Relative, [int] $X, [int] $Y, [int] $Width, [int] $Height)
    $image = [System.Drawing.Image]::FromFile((Resolve-SceneSource $Relative))
    $state = $Graphics.Save()
    try {
        $Graphics.SetClip([System.Drawing.Rectangle]::new($X, $Y, $Width, $Height))
        for ($top = $Y; $top -lt ($Y + $Height); $top += $image.Height) {
            for ($left = $X; $left -lt ($X + $Width); $left += $image.Width) {
                $Graphics.DrawImageUnscaled($image, $left, $top)
            }
        }
    } finally {
        $Graphics.Restore($state)
        $image.Dispose()
    }
}

function Convert-SceneToSepia {
    param([Parameter(Mandatory)] [System.Drawing.Bitmap] $Canvas)
    for ($y = 0; $y -lt $Canvas.Height; $y += 1) {
        for ($x = 0; $x -lt $Canvas.Width; $x += 1) {
            $pixel = $Canvas.GetPixel($x, $y)
            $luma = [int][Math]::Round((0.299 * $pixel.R) + (0.587 * $pixel.G) + (0.114 * $pixel.B))
            $red = [Math]::Clamp([int][Math]::Round(($luma * 0.88) + 42), 0, 255)
            $green = [Math]::Clamp([int][Math]::Round(($luma * 0.68) + 27), 0, 255)
            $blue = [Math]::Clamp([int][Math]::Round(($luma * 0.40) + 12), 0, 255)
            $Canvas.SetPixel($x, $y, [System.Drawing.Color]::FromArgb($pixel.A, $red, $green, $blue))
        }
    }
}

function Draw-RoaringKnightRune {
    param(
        $Graphics,
        [double] $X,
        [double] $Y,
        [double] $Size,
        [double] $Angle,
        [int] $Frame,
        [int] $Variant
    )

    $state = $Graphics.Save()
    try {
        $Graphics.TranslateTransform([single]$X, [single]$Y)
        $Graphics.RotateTransform([single]$Angle)
        $scale = [single]($Size / 48.0)
        $Graphics.ScaleTransform($scale, $scale)

        $outer = [System.Drawing.PointF[]]@(
            [System.Drawing.PointF]::new(0, -23),
            [System.Drawing.PointF]::new(7, -13),
            [System.Drawing.PointF]::new(20, -12),
            [System.Drawing.PointF]::new(14, -3),
            [System.Drawing.PointF]::new(23, 0),
            [System.Drawing.PointF]::new(14, 4),
            [System.Drawing.PointF]::new(20, 13),
            [System.Drawing.PointF]::new(7, 12),
            [System.Drawing.PointF]::new(0, 23),
            [System.Drawing.PointF]::new(-7, 12),
            [System.Drawing.PointF]::new(-20, 13),
            [System.Drawing.PointF]::new(-14, 4),
            [System.Drawing.PointF]::new(-23, 0),
            [System.Drawing.PointF]::new(-14, -3),
            [System.Drawing.PointF]::new(-20, -12),
            [System.Drawing.PointF]::new(-7, -13)
        )
        $inner = [System.Drawing.PointF[]]@(
            [System.Drawing.PointF]::new(0, -9),
            [System.Drawing.PointF]::new(9, 0),
            [System.Drawing.PointF]::new(0, 9),
            [System.Drawing.PointF]::new(-9, 0)
        )
        $alpha = 145 + (($Frame + ($Variant * 7)) % 4) * 18
        $outline = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb($alpha, 224, 219, 230), 1.4)
        $innerPen = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb([Math]::Min(255, $alpha + 25), 245, 240, 246), 1.1)
        try {
            $Graphics.DrawPolygon($outline, $outer)
            $Graphics.DrawPolygon($innerPen, $inner)
            $Graphics.DrawLine($innerPen, -15, -12, -8, -5)
            $Graphics.DrawLine($innerPen, 15, -12, 8, -5)
            $Graphics.DrawLine($innerPen, -15, 12, -8, 5)
            $Graphics.DrawLine($innerPen, 15, 12, 8, 5)
        } finally {
            $outline.Dispose()
            $innerPen.Dispose()
        }
    } finally {
        $Graphics.Restore($state)
    }
}

function Draw-RoaringKnightBattleScene {
    param($Graphics, [int] $Frame)

    Set-SceneColor $Graphics '#050000'
    $cycle = (2.0 * [Math]::PI * $Frame) / 144.0

    for ($ring = 8; $ring -ge 0; $ring -= 1) {
        $diameter = 42 + ($ring * 23) + ([Math]::Sin($cycle + $ring) * 5)
        $red = [Math]::Clamp(52 + ($ring * 6), 0, 120)
        $brush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(9, $red, 2, 5))
        try {
            $Graphics.FillEllipse($brush, [single](160 - ($diameter / 2)), [single](86 - ($diameter / 2)), [single]$diameter, [single]$diameter)
        } finally {
            $brush.Dispose()
        }
    }

    for ($petal = 0; $petal -lt 14; $petal += 1) {
        $angle = $cycle + (($petal / 14.0) * 2.0 * [Math]::PI)
        $radius = 28 + (($petal % 4) * 13)
        $x = 160 + ([Math]::Cos($angle) * $radius)
        $y = 86 + ([Math]::Sin($angle) * $radius * 0.62)
        $width = 58 + (($petal % 3) * 15)
        $height = 24 + (($petal % 4) * 7)
        $brush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(29, 112 + (($petal % 3) * 18), 3, 7))
        try {
            $Graphics.FillEllipse($brush, [single]($x - ($width / 2)), [single]($y - ($height / 2)), [single]$width, [single]$height)
        } finally {
            $brush.Dispose()
        }
    }

    $glowBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(48, 196, 32, 38))
    $coreBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(25, 240, 220, 220))
    try {
        $Graphics.FillEllipse($glowBrush, 105, 20, 110, 126)
        $Graphics.FillEllipse($coreBrush, 133, 34, 54, 84)
    } finally {
        $glowBrush.Dispose()
        $coreBrush.Dispose()
    }

    $stars = @(
        @(34, 42, 19, 12, 34, 0.15, 1),
        @(286, 22, 22, 9, 30, 1.05, -1),
        @(280, 88, 18, 16, 38, 2.20, 1),
        @(270, 151, 22, 12, 35, 3.10, -1),
        @(48, 151, 18, 12, 34, 4.15, 1),
        @(139, 145, 25, 10, 29, 5.20, -1),
        @(-4, 91, 20, 15, 32, 0.72, 1)
    )
    for ($index = 0; $index -lt $stars.Count; $index += 1) {
        $star = $stars[$index]
        $phaseOffset = [double]$star[5]
        $x = [double]$star[0] + ([Math]::Sin($cycle + $phaseOffset) * [double]$star[2])
        $y = [double]$star[1] + ([Math]::Cos(($cycle * 2.0) + ($phaseOffset * 1.35)) * [double]$star[3])
        $rotation = (($Frame * 2.5 * [double]$star[6]) + ($index * 27)) % 360
        Draw-RoaringKnightRune $Graphics $x $y ([double]$star[4]) $rotation $Frame $index
    }

    $knightY = 52 + [Math]::Round([Math]::Sin($cycle * 2.0) * 2.0)
    $knightPath = Resolve-SceneSource 'deltarune-ch4/Sprites/spr_roaringknight_ball_transition_pose_4.png'
    $knight = [System.Drawing.Bitmap]::FromFile($knightPath)
    try {
        # The source pose is intentionally a single frame. The game-like idle comes
        # from deterministic horizontal row displacement rather than soft scaling.
        $targetX = 129
        $targetWidth = 62
        $targetHeight = 72
        $glitchPhase = $Frame % 72
        for ($sourceY = 0; $sourceY -lt $knight.Height; $sourceY += 1) {
            # Every temporal term uses an integer multiple of the shared cycle,
            # so frame 143 advances naturally into frame 0 without a pose jump.
            $wave = [Math]::Round(
                ([Math]::Sin(($sourceY * 0.31) + ($cycle * 5.0)) * 1.15) +
                ([Math]::Sin(($sourceY * 0.09) - ($cycle * 2.0)) * 0.65)
            )
            if ($glitchPhase -ge 31 -and $glitchPhase -le 35) {
                $band = [Math]::Floor($sourceY / 6)
                $wave += (($band + $glitchPhase) % 5) - 2
            }

            $destinationTop = $knightY + [Math]::Floor(($sourceY * $targetHeight) / $knight.Height)
            $destinationBottom = $knightY + [Math]::Floor((($sourceY + 1) * $targetHeight) / $knight.Height)
            $destinationHeight = [Math]::Max(1, $destinationBottom - $destinationTop)
            $destination = [System.Drawing.Rectangle]::new(
                $targetX + [int]$wave,
                [int]$destinationTop,
                $targetWidth,
                [int]$destinationHeight
            )
            $Graphics.DrawImage(
                $knight,
                $destination,
                0,
                $sourceY,
                $knight.Width,
                1,
                [System.Drawing.GraphicsUnit]::Pixel
            )
        }
    } finally {
        $knight.Dispose()
    }

}

function Write-ThemeBackground {
    param([Parameter(Mandatory)] [string] $Scene, [Parameter(Mandatory)] [string] $Destination)

    $selectedScene = @{
        'undertale-ruins' = 'selected-scenes/undertale-ruins.png'
        'undertale-snowdin' = 'selected-scenes/undertale-snowdin.png'
        'undertale-waterfall' = 'selected-scenes/undertale-waterfall.png'
        'undertale-void' = 'selected-scenes/undertale-barrier.png'
        'undertale-hotland' = 'selected-scenes/undertale-hotland.png'
        'undertale-core' = 'selected-scenes/undertale-core.png'
        'undertale-true-lab' = 'selected-scenes/undertale-true-lab.png'
        'undertale-new-home' = 'selected-scenes/undertale-new-home.png'
    }[$Scene]
    if ($selectedScene) {
        Copy-Item -LiteralPath (Resolve-SceneSource $selectedScene) -Destination $Destination -Force
        return
    }

    $canvas = [System.Drawing.Bitmap]::new(320, 180, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [System.Drawing.Graphics]::FromImage($canvas)
    try {
        $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceOver
        $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighSpeed
        $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
        $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
        $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::None

        switch ($Scene) {
            'card-castle' {
                Set-SceneColor $graphics '#050719'
                Draw-SceneCrop $graphics 'deltarune-ch1/Sprites/bg_cardcastle_outside_0.png' 0 40 640 360
            }
            'noelle' {
                Set-SceneColor $graphics '#07131E'
                Draw-SceneCrop $graphics 'deltarune-ch2/Sprites/bg_dw_noelle_room_0.png' 180 0 853 480
                Draw-SceneAsset $graphics 'deltarune-ch2/Sprites/spr_noelleb_spell_4.png' 225 70 66 96
            }
            'tv-world' {
                Set-SceneColor $graphics '#251037'
                Draw-SceneAsset $graphics 'deltarune-ch3/Sprites/spr_dw_gameshow_bg_0.png' 0 0 320 180
                Draw-SceneAsset $graphics 'deltarune-ch3/Sprites/spr_gameshow_screen_city_0.png' 48 12 224 101
                Draw-SceneAsset $graphics 'deltarune-ch3/Sprites/spr_dw_gameshow_tv_frame_0.png' 0 0 320 120
                Draw-SceneAsset $graphics 'deltarune-ch3/Sprites/spr_gameshow_screen_logo_0.png' 12 8 78 35
                Draw-SceneAsset $graphics 'deltarune-ch3/Sprites/spr_dw_gameshow_podium_0.png' 88 124 35 47
                Draw-SceneAsset $graphics 'deltarune-ch3/Sprites/spr_dw_gameshow_podium_1.png' 130 124 35 47
                Draw-SceneAsset $graphics 'deltarune-ch3/Sprites/spr_dw_gameshow_podium_2.png' 172 124 35 47
                Draw-SceneAsset $graphics 'deltarune-ch3/Sprites/spr_gameshow_crowd_a_0.png' 0 132 55 48
                Draw-SceneAsset $graphics 'deltarune-ch3/Sprites/spr_gameshow_crowd_b_0.png' 265 136 55 44
                Draw-SceneAsset $graphics 'deltarune-ch3/Sprites/spr_tenna_armsup_annoyed_0.png' 238 88 58 75
            }
            'the-knight' {
                Draw-RoaringKnightBattleScene $graphics 0
            }
            'undertale-snowdin' {
                Set-SceneColor $graphics '#0B2439'
                Draw-SceneAsset $graphics 'undertale/Sprites/spr_singletree_0.png' 12 38 60 60
                Draw-SceneAsset $graphics 'undertale/Sprites/spr_singletree_0.png' 246 34 60 60
                Fill-SceneRectangle $graphics '#D8F2FF' 0 154 320 26
                Draw-SceneAsset $graphics 'undertale/Backgrounds/bg_library_outside_0.png' 0 87 142 79
                Draw-SceneAsset $graphics 'undertale/Backgrounds/bg_inn_shop_0.png' 82 83 170 77
                Draw-SceneAsset $graphics 'undertale/Backgrounds/bg_snowdinhouse_0.png' 232 78 88 81
            }
            'undertale-waterfall' {
                Set-SceneColor $graphics '#020822'
                Tile-SceneAsset $graphics 'undertale/Sprites/spr_waterfall_midmid_0.png' 0 0 320 180
                Tile-SceneAsset $graphics 'undertale/Sprites/spr_waterfall_bright_mm_0.png' 0 0 20 180
                Tile-SceneAsset $graphics 'undertale/Sprites/spr_waterfall_bright_mm_0.png' 300 0 20 180
                Draw-SceneAsset $graphics 'undertale/Backgrounds/bg_waterfall_castle_0.png' 56 5 208 168
                Draw-SceneAsset $graphics 'undertale/Sprites/spr_echoflower_0.png' 34 138 20 30
                Draw-SceneAsset $graphics 'undertale/Sprites/spr_echoflower_0.png' 266 138 20 30
                Draw-SceneAsset $graphics 'undertale/Sprites/spr_glowshroom_0.png' 75 148 20 20
                Draw-SceneAsset $graphics 'undertale/Sprites/spr_glowshroom_0.png' 225 148 20 20
            }
            'undertale-void' {
                Set-SceneColor $graphics '#000000'
                Fill-SceneRectangle $graphics '#101010' 0 132 320 2
                Fill-SceneRectangle $graphics '#080808' 0 160 320 20
                Draw-SceneAsset $graphics 'undertale/Sprites/spr_greydoor_0.png' 28 50 72 104
                Draw-SceneAsset $graphics 'undertale/Sprites/spr_g_follower_1_0.png' 111 111 30 45
                Draw-SceneAsset $graphics 'undertale/Sprites/spr_mysteryman_0.png' 148 48 48 106
                Draw-SceneAsset $graphics 'undertale/Sprites/spr_g_follower_2_0.png' 216 87 60 69
                Draw-SceneAsset $graphics 'undertale/Sprites/spr_g_follower_3_0.png' 276 96 36 40
            }
            'undertale-hotland' {
                Set-SceneColor $graphics '#250603'
                Tile-SceneAsset $graphics 'undertale/Backgrounds/bg_lavatile4x4_0.png' 0 100 320 80
                Draw-SceneAsset $graphics 'undertale/Backgrounds/bg_alphyslabl_new_0.png' 76 0 164 180
                Tile-SceneAsset $graphics 'undertale/Sprites/spr_hotland_rededge_0.png' 0 92 320 40
            }
            'undertale-core' {
                Set-SceneColor $graphics '#000311'
                Draw-SceneCrop $graphics 'undertale/Backgrounds/bg_core_distance_0.png' 0 0 320 180
                Draw-SceneCrop $graphics 'undertale/Backgrounds/bg_core_distance_foreground_0.png' 0 0 320 180
            }
        }

        $output = [System.Drawing.Bitmap]::new(1280, 720, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $outputGraphics = [System.Drawing.Graphics]::FromImage($output)
        try {
            $outputGraphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
            $outputGraphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
            $outputGraphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::None
            $outputGraphics.DrawImage($canvas, [System.Drawing.Rectangle]::new(0, 0, 1280, 720))
            $output.Save($Destination, [System.Drawing.Imaging.ImageFormat]::Png)
        } finally {
            $outputGraphics.Dispose()
            $output.Dispose()
        }
    } finally {
        $graphics.Dispose()
        $canvas.Dispose()
    }
}

function Write-RoaringKnightAnimation {
    param([Parameter(Mandatory)] [string] $Destination)

    $ffmpeg = Get-Command ffmpeg -CommandType Application -ErrorAction Stop
    $frameDirectory = Join-Path ([System.IO.Path]::GetTempPath()) ("deltamod-roaring-knight-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $frameDirectory | Out-Null
    try {
        foreach ($index in 0..143) {
            $canvas = [System.Drawing.Bitmap]::new(320, 180, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
            $graphics = [System.Drawing.Graphics]::FromImage($canvas)
            try {
                $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceOver
                $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
                $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
                $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::None
                Draw-RoaringKnightBattleScene $graphics $index

                $output = [System.Drawing.Bitmap]::new(1280, 720, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
                $outputGraphics = [System.Drawing.Graphics]::FromImage($output)
                try {
                    $outputGraphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
                    $outputGraphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
                    $outputGraphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::None
                    $outputGraphics.DrawImage($canvas, [System.Drawing.Rectangle]::new(0, 0, 1280, 720))
                    $framePath = Join-Path $frameDirectory ("frame-{0:D3}.png" -f $index)
                    $output.Save($framePath, [System.Drawing.Imaging.ImageFormat]::Png)
                } finally {
                    $outputGraphics.Dispose()
                    $output.Dispose()
                }
            } finally {
                $graphics.Dispose()
                $canvas.Dispose()
            }
        }

        & $ffmpeg.Source -hide_banner -loglevel error -y -framerate 24 -i (Join-Path $frameDirectory 'frame-%03d.png') -c:v libx264 -preset slow -crf 12 -g 24 -keyint_min 24 -sc_threshold 0 -bf 0 -pix_fmt yuv420p -movflags '+faststart' -an $Destination
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $Destination -PathType Leaf)) {
            throw "Failed to encode Roaring Knight background video: ffmpeg exit $LASTEXITCODE"
        }
    } finally {
        if (Test-Path -LiteralPath $frameDirectory -PathType Container) {
            Remove-Item -LiteralPath $frameDirectory -Recurse -Force
        }
    }
}

$provenance = @()
foreach ($spec in $specs) {
    $sceneSources = @(Get-SceneSources -Scene $spec.Scene)
    $resolvedSceneSources = @($sceneSources | ForEach-Object { Resolve-SceneSource $_ })
    $gameRoot = if ($spec.Work -eq 'DELTARUNE') { $deltarune } else { $undertale }
    $music = Join-Path $gameRoot $spec.Music
    if (-not (Test-Path -LiteralPath $music -PathType Leaf)) { throw "Missing music source: $($spec.Music)" }
    $header = [System.IO.File]::ReadAllBytes($music)
    if ($header.Length -lt 4 -or [Text.Encoding]::ASCII.GetString($header, 0, 4) -ne 'OggS') {
        throw "Music source is not Ogg: $($spec.Music)"
    }

    $backgroundName = "$($spec.Id).png"
    $musicName = "$($spec.Id).ogg"
    $backgroundOutput = Join-Path $imageDirectory $backgroundName
    $musicOutput = Join-Path $musicDirectory $musicName
    $manifestOutput = Join-Path $dataDirectory "$($spec.Id).theme.json"
    Write-ThemeBackground -Scene $spec.Scene -Destination $backgroundOutput
    Copy-Item -LiteralPath $music -Destination $musicOutput -Force
    $videoOutput = $null
    if ($spec.Video) {
        $videoOutput = Join-Path $videoDirectory $spec.Video
        Write-RoaringKnightAnimation -Destination $videoOutput
    }

    $manifest = [ordered]@{
        name = $spec.Name
        background = $backgroundName
        description = $spec.Description
        mainSong = $musicName
        musicTrack = "$($spec.Track) - Toby Fox"
        id = $spec.Id
        color = $spec.Color
        soulColor = $spec.Soul
    }
    if ($spec.Video) {
        $manifest.backgroundVideo = $spec.Video
        $manifest.videoHasAudio = $false
    }
    [System.IO.File]::WriteAllText(
        $manifestOutput,
        ($manifest | ConvertTo-Json -Depth 5),
        [System.Text.UTF8Encoding]::new($false)
    )
    $themeProvenance = [ordered]@{
        id = $spec.Id
        sourceWork = $spec.Work
        sceneSources = @($sceneSources | ForEach-Object -Begin { $index = 0 } -Process {
            $source = $resolvedSceneSources[$index]
            $index += 1
            [ordered]@{
                identifier = $_
                sha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
            }
        })
        musicIdentifier = $spec.Music
        musicSha256 = (Get-FileHash -LiteralPath $music -Algorithm SHA256).Hash.ToLowerInvariant()
        backgroundSha256 = (Get-FileHash -LiteralPath $backgroundOutput -Algorithm SHA256).Hash.ToLowerInvariant()
        bundledMusicSha256 = (Get-FileHash -LiteralPath $musicOutput -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    if ($videoOutput) {
        $themeProvenance.backgroundVideo = $spec.Video
        $themeProvenance.backgroundVideoSha256 = (Get-FileHash -LiteralPath $videoOutput -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $provenance += $themeProvenance
    Write-Host "Generated bundled theme $($spec.Id)"
}

if ($ProvenanceOutput) {
    $parent = Split-Path -Parent $ProvenanceOutput
    if ($parent -and -not (Test-Path -LiteralPath $parent -PathType Container)) {
        throw "Missing provenance output directory: $parent"
    }
    $provenanceThemes = $provenance
    if ($requestedThemeIds.Count -gt 0 -and (Test-Path -LiteralPath $ProvenanceOutput -PathType Leaf)) {
        $existingDocument = Get-Content -LiteralPath $ProvenanceOutput -Raw | ConvertFrom-Json
        $replacementById = @{}
        foreach ($entry in $provenance) { $replacementById[$entry.id] = $entry }
        $provenanceThemes = @($existingDocument.themes | ForEach-Object {
            if ($replacementById.ContainsKey($_.id)) { $replacementById[$_.id] } else { $_ }
        })
    }
    $document = [ordered]@{
        schemaVersion = 2
        generator = 'scripts/generate-bundled-game-themes.ps1'
        themes = $provenanceThemes
    }
    [System.IO.File]::WriteAllText(
        $ProvenanceOutput,
        ($document | ConvertTo-Json -Depth 8),
        [System.Text.UTF8Encoding]::new($false)
    )
}
