param(
  [string]$Wav = "",
  [Parameter(Mandatory = $true)][string]$OutDir,
  [string]$Name = "clip.mp4",
  [int]$Seconds = 6,
  [string]$Quality = "Qvga",
  [int]$Channels = 0
)
# Makes a real MP4 (H.264 video + AAC audio) with the encoders that ship with Windows.
Add-Type -AssemblyName System.Runtime.WindowsRuntime
foreach ($t in @(
  "Windows.Storage.StorageFile, Windows.Storage",
  "Windows.Storage.StorageFolder, Windows.Storage",
  "Windows.Storage.CreationCollisionOption, Windows.Storage",
  "Windows.Media.Editing.MediaComposition, Windows.Media.Editing",
  "Windows.Media.Editing.MediaClip, Windows.Media.Editing",
  "Windows.Media.Editing.BackgroundAudioTrack, Windows.Media.Editing",
  "Windows.Media.Editing.MediaTrimmingPreference, Windows.Media.Editing",
  "Windows.Media.MediaProperties.MediaEncodingProfile, Windows.Media.MediaProperties",
  "Windows.Media.MediaProperties.VideoEncodingQuality, Windows.Media.MediaProperties",
  "Windows.Media.Transcoding.TranscodeFailureReason, Windows.Media.Transcoding",
  "Windows.UI.Colors, Windows.UI"
)) {
  $typeName, $asm = $t -split ", ", 2
  $null = Invoke-Expression "[$typeName, $asm, ContentType = WindowsRuntime]"
}
$ext = [System.WindowsRuntimeSystemExtensions].GetMethods()
$asTaskOp = $ext | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' } | Select-Object -First 1
$asTaskOpP = $ext | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperationWithProgress`2' } | Select-Object -First 1
function Await-Op($op, [Type]$result) {
  $task = $asTaskOp.MakeGenericMethod($result).Invoke($null, @($op))
  $task.Wait(-1) | Out-Null
  $task.Result
}
$folder = Await-Op ([Windows.Storage.StorageFolder]::GetFolderFromPathAsync($OutDir)) ([Windows.Storage.StorageFolder])
$out = Await-Op ($folder.CreateFileAsync($Name, [Windows.Storage.CreationCollisionOption]::ReplaceExisting)) ([Windows.Storage.StorageFile])
$comp = New-Object Windows.Media.Editing.MediaComposition
$clip = [Windows.Media.Editing.MediaClip]::CreateFromColor([Windows.UI.Colors]::Teal, [TimeSpan]::FromSeconds($Seconds))
# The WinRT vectors do not expose Add to PowerShell directly: go through the .NET interface.
[System.Collections.Generic.ICollection[Windows.Media.Editing.MediaClip]].GetMethod("Add").Invoke($comp.Clips, @($clip))
if ($Wav) {
  $wavFile = Await-Op ([Windows.Storage.StorageFile]::GetFileFromPathAsync($Wav)) ([Windows.Storage.StorageFile])
  $track = Await-Op ([Windows.Media.Editing.BackgroundAudioTrack]::CreateFromFileAsync($wavFile)) ([Windows.Media.Editing.BackgroundAudioTrack])
  [System.Collections.Generic.ICollection[Windows.Media.Editing.BackgroundAudioTrack]].GetMethod("Add").Invoke($comp.BackgroundAudioTracks, @($track))
}
$profile = [Windows.Media.MediaProperties.MediaEncodingProfile]::CreateMp4([Windows.Media.MediaProperties.VideoEncodingQuality]::$Quality)
if ($Channels -gt 0) { $profile.Audio.ChannelCount = $Channels; $profile.Audio.SampleRate = 48000; $profile.Audio.Bitrate = 64000 * $Channels }
$op = $comp.RenderToFileAsync($out, [Windows.Media.Editing.MediaTrimmingPreference]::Fast, $profile)
$task = $asTaskOpP.MakeGenericMethod([Windows.Media.Transcoding.TranscodeFailureReason], [double]).Invoke($null, @($op))
$task.Wait(-1) | Out-Null
"render result: $($task.Result)"
Get-Item (Join-Path $OutDir $Name) | Select-Object Name, Length
