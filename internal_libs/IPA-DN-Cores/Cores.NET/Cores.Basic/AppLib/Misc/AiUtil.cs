// IPA Cores.NET
// 
// Copyright (c) 2019- IPA CyberLab.
// Copyright (c) 2003-2018 Daiyuu Nobori.
// Copyright (c) 2013-2018 SoftEther VPN Project, University of Tsukuba, Japan.
// All Rights Reserved.
// 
// License: The Apache License, Version 2.0
// https://www.apache.org/licenses/LICENSE-2.0
// 
// THIS SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
// 
// THIS SOFTWARE IS DEVELOPED IN JAPAN, AND DISTRIBUTED FROM JAPAN, UNDER
// JAPANESE LAWS. YOU MUST AGREE IN ADVANCE TO USE, COPY, MODIFY, MERGE, PUBLISH,
// DISTRIBUTE, SUBLICENSE, AND/OR SELL COPIES OF THIS SOFTWARE, THAT ANY
// JURIDICAL DISPUTES WHICH ARE CONCERNED TO THIS SOFTWARE OR ITS CONTENTS,
// AGAINST US (IPA CYBERLAB, DAIYUU NOBORI, SOFTETHER VPN PROJECT OR OTHER
// SUPPLIERS), OR ANY JURIDICAL DISPUTES AGAINST US WHICH ARE CAUSED BY ANY KIND
// OF USING, COPYING, MODIFYING, MERGING, PUBLISHING, DISTRIBUTING, SUBLICENSING,
// AND/OR SELLING COPIES OF THIS SOFTWARE SHALL BE REGARDED AS BE CONSTRUED AND
// CONTROLLED BY JAPANESE LAWS, AND YOU MUST FURTHER CONSENT TO EXCLUSIVE
// JURISDICTION AND VENUE IN THE COURTS SITTING IN TOKYO, JAPAN. YOU MUST WAIVE
// ALL DEFENSES OF LACK OF PERSONAL JURISDICTION AND FORUM NON CONVENIENS.
// PROCESS MAY BE SERVED ON EITHER PARTY IN THE MANNER AUTHORIZED BY APPLICABLE
// LAW OR COURT RULE.

// Author: Daiyuu Nobori
// 一部のコード: ChatGPT o1 Pro が生成
// Description

#if true

#pragma warning disable CA2235 // Mark all non-serializable fields

using System;
using System.Buffers;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Collections.Generic;
using System.Collections.Concurrent;
using System.Collections.Immutable;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Runtime.InteropServices;
using System.Runtime.CompilerServices;
using System.Diagnostics.CodeAnalysis;
using System.Runtime.Serialization;

using NAudio.Wave;
using NAudio.Utils; // WaveFileWriter で MemoryStream を使うため (IgnoreDisposeStream)

using IPA.Cores.Basic;
using IPA.Cores.Helper.Basic;
using static IPA.Cores.Globals.Basic;
using System.Text.RegularExpressions;
using System.Buffers.Binary;
using Newtonsoft.Json;
using Newtonsoft.Json.Converters;
using Newtonsoft.Json.Linq;
using IPA.Cores.Basic.HttpClientCore;
using System.Net;

namespace IPA.Cores.Basic;

public static partial class CoresConfig
{
    public static partial class DefaultAiUtilSettings
    {
        public static readonly Copenhagen<int> DefaultMaxStdOutBufferSize = 2_000_000_000;
        public static readonly Copenhagen<double> AdjustAudioTargetMaxVolume = 0.0;
        public static readonly Copenhagen<double> AdjustAudioTargetMeanVolume = -15.0;
    }
}

public static class AiUtilVersion
{
    public const int CurrentVersion = 20250330_04;
}

public class AiCompositWaveSettings
{
    public List<AiCompositRule> RulesList = null!;

    [JsonIgnore]
    public Func<AiTaskOperationDesc, CancellationToken, AiAudioEffectFilter?>? CreateAudioEffectFilter = null;
}

public class AiCompositWaveParam
{
    public double MarginSecs = 30;
    public double StdFadeInSecs = 2;
    public double StdFadeOutSecs = 13;
    public double MaxRandBeforeLengthSecs = 0;
    public double MaxRandAfterLengthSecs = 13;
    public double VolumeDelta = -17;
    public double VolumeDeltaRandomRange = 10;
    public double DurationWhenEndTagIsEmpty = 5;
    public AiAudioEffectSpeedType? EffectType = null;
}

public class AiCompositRuleData
{
    public AiCompositWaveParam Param = new();
    public IEndlessQueue<string> MaterialsWavPathQueue = null!;
    public int MultipleMode_Count = 0;
    public double Multiplemode_SpanRatio = 0;
}

public class AiCompositRule
{
    public string StartTagStr = Str.NewGuid();
    public string EndTagStr = Str.NewGuid();
    public Func<AiCompositRuleData> GetRuleFunc = null!;

    public AiCompositRuleData RuleData => _RuleDataCreatedInternal;
    readonly CachedProperty<AiCompositRuleData> _RuleDataCreatedInternal;

    public AiCompositRule()
    {
        this._RuleDataCreatedInternal = new(getter: () => this.GetRuleFunc());
    }
}

public class AiAudioEffectFilter
{
    public AiAudioEffectBase Effect = null!;
    public Action<Memory<byte>, CancellationToken> PerformFilterFunc = null!;
    public string FilterName = "";
    public AiAudioEffectSpeedType FilterSpeedType = AiAudioEffectSpeedType.Normal;
    public IAiAudioEffectSettings EffectSettings = null!;

    public AiAudioEffectFilter(AiAudioEffectBase effect, AiAudioEffectSpeedType type, double adjustAddVolume = 0.0, CancellationToken cancel = default)
    {
        this.Effect = effect;
        string name = PP.GetExtension(effect.ToString()._NotEmptyOrDefault("_unknown"));
        if (name.StartsWith(".")) name = name.Substring(1);
        if (name._IsEmpty()) name = "_unknown";

        this.FilterName = PP.MakeSafeFileName(name);
        this.FilterSpeedType = type;
        this.EffectSettings = effect.NewSettingsFactoryWithRandom(type);
        this.PerformFilterFunc = (wave, cancel) =>
        {
            var originalVolume = AiWaveVolumeUtils.CalcMeanVolume(wave, cancel);

            this.Effect.ProcessFilter(wave, this.EffectSettings, cancel);

            AiWaveVolumeUtils.AdjustVolume(wave, originalVolume + adjustAddVolume, cancel);
        };
    }
}

public class AiVoiceFilterRule
{
    public string StartTagStr = Str.NewGuid();
    public string EndTagStr = Str.NewGuid();
    public Func<AiAudioEffectFilter?>? CreateFilterFunc = null!;
}

public class AiTaskOperationDesc
{
    public double StartPosition = 0;
    public double EndPosition = 0;
    public AiCompositRule Rule = null!;
    public IEndlessQueue<string> MatFilesQueue = null!;

    public string Calced_MaterialWavPath = "";
    public double Calced_TargetPositionSecs;
    public double Calced_MeterialPositionSecs;
    public double Calced_LengthSecs;
    public double Calced_FadeInSecs;
    public double Calced_FadeOutSecs;
    public double Calced_VolumeDelta_Left;
    public double Calced_VolumeDelta_Right;

    public AiAudioEffectFilter? Filter = null;
}

[Flags]
public enum AiTaskTtsEngine
{
    VoiceVox = 0,
    FishAudioPlain,
    FishAudioWithSeedVc,
}

public class AiTask
{
    public static readonly RefLong TotalGpuProcessTimeMsecs = new RefLong();

    public FfMpegUtil FfMpeg { get; }
    public AiUtilBasicSettings Settings { get; }

    public AiTask(AiUtilBasicSettings settings, FfMpegUtil ffMpeg)
    {
        this.Settings = settings;
        this.FfMpeg = ffMpeg;
    }

    public class RandomSegmentMetaData
    {
        public string SrcFileName = "";
        public int DurationMsecs;
        public int TotalLengthMsecs;
        public int Number;
        public int TotalCount;
    }

    public async Task ExtractRandomVocalSegmentsFromAllWavAsync(string wavFileDir, string dstDirName, int durationMsecs, CancellationToken cancel = default)
    {
        var wavFilesList = await Lfs.EnumDirectoryAsync(wavFileDir, wildcard: "*.wav", cancel: cancel);

        foreach (var wavFile in wavFilesList.Where(x => x.IsFile)._Shuffle().ToList())
        {
            string[] tokens = wavFile.Name._Split(StringSplitOptions.None, " - ");
            if (tokens.Length >= 3 && tokens[0]._IsSamei("vocalonly"))
            {
                string artistName = tokens[1].Replace("_", "");

                if (artistName._IsFilled())
                {
                    Con.WriteLine(wavFile.FullPath);
                    await ExtractRandomSegmentsFromWavAsync(wavFile.FullPath, dstDirName, artistName, durationMsecs, cancel);
                }
            }
        }
    }

    public async Task ExtractRandomSpeechSegmentsFromAllWavAsync(string wavFileDir, string dstDirName, int durationMsecs, CancellationToken cancel = default)
    {
        var wavFilesList = await Lfs.EnumDirectoryAsync(wavFileDir, wildcard: "*.wav", cancel: cancel);

        foreach (var wavFile in wavFilesList.Where(x => x.IsFile).OrderBy(x => x.Name, StrCmpi))
        {
            string[] tokens = wavFile.Name._Split(StringSplitOptions.None, " - ");
            if (tokens.Length >= 3 && tokens[0]._IsSamei("vocalonly"))
            {
                string tmp1 = tokens[2];

                string[] tokens2 = tmp1._Split(StringSplitOptions.None, "_");
                if (tokens2.Length >= 2)
                {
                    string prefix = tokens2[0].Trim();
                    if (prefix._IsFilled())
                    {
                        Con.WriteLine(wavFile.FullPath);
                        await ExtractRandomSegmentsFromWavAsync(wavFile.FullPath, dstDirName, prefix, durationMsecs, cancel);
                    }
                }
            }
        }
    }

    public async Task ExtractRandomSegmentsFromWavAsync(string wavFilePath, string dstDirName, string dstBaseFileName, int durationMsecs, CancellationToken cancel = default)
    {
        int waveLengthMsecs;
        await using (var reader = new WaveFileReader(wavFilePath))
        {
            waveLengthMsecs = (int)reader.TotalTime.TotalMilliseconds;
        }

        if (durationMsecs > waveLengthMsecs)
        {
            durationMsecs = waveLengthMsecs;
        }

        await Lfs.CreateDirectoryAsync(dstDirName, cancel: cancel);

        int count;
        if (waveLengthMsecs <= 1.5 * durationMsecs)
            count = 1;
        else if (waveLengthMsecs <= 2.0 * durationMsecs)
            count = 2;
        else if (waveLengthMsecs <= 3.0 * durationMsecs)
            count = 3;
        else if (waveLengthMsecs <= 4.0 * durationMsecs)
            count = 4;
        else
            count = 5;

        Random rand = new Random(Secure.RandSInt31());

        for (int i = 1; i <= count; i++)
        {
            int startMsec = rand.Next(waveLengthMsecs - durationMsecs + 1);

            await using (var reader = new WaveFileReader(wavFilePath))
            {
                reader.CurrentTime = TimeSpan.FromMilliseconds(startMsec);

                string outputPath;

                for (int j = 1; ; j++)
                {
                    string outputFileNameTmp = $"{dstBaseFileName}_{j.ToString("D3")}.wav";
                    string outputFilePathTmp = PP.Combine(dstDirName, outputFileNameTmp);

                    if (await Lfs.IsFileExistsAsync(outputFilePathTmp, cancel) == false || await Lfs.IsOkFileExistsAsync(outputFilePathTmp, cancel: cancel) == false)
                    {
                        outputPath = outputFilePathTmp;
                        break;
                    }
                }

                await using (var writer = new WaveFileWriter(outputPath, reader.WaveFormat))
                {
                    int bytesPerMillisecond = reader.WaveFormat.AverageBytesPerSecond / 1000;
                    int bytesToRead = bytesPerMillisecond * durationMsecs;

                    byte[] buffer = new byte[65536];
                    int bytesRemaining = bytesToRead;

                    while (bytesRemaining > 0)
                    {
                        int readBytes = await reader.ReadAsync(buffer, 0, Math.Min(buffer.Length, bytesRemaining), cancel);
                        if (readBytes == 0)
                        {
                            break;
                        }

                        await writer.WriteAsync(buffer, 0, readBytes, cancel);
                        bytesRemaining -= readBytes;
                    }
                }

                var meta = new RandomSegmentMetaData
                {
                    DurationMsecs = durationMsecs,
                    TotalLengthMsecs = waveLengthMsecs,
                    SrcFileName = wavFilePath,
                    Number = i,
                    TotalCount = count,
                };

                await Lfs.WriteOkFileAsync(outputPath, meta, "", 0, cancel);
            }
        }
    }

    public static async Task<int> GetWavFileLengthMSecAsync(string wavFilePath)
    {
        checked
        {
            await using (var reader = new WaveFileReader(wavFilePath))
            {
                return (int)reader.TotalTime.TotalMilliseconds;
            }
        }
    }

    public async Task GenerateRandomSampleOfTextToVoiceAsync(int maxTryCount, string srcText, int textLengthOfRandomPart, string sampleVoiceWavDirName, int speakerId, int diffusionSteps, string dstVoiceDirPath, string tmpVoiceBoxDir, string tmpVoiceWavDir, string testName, CancellationToken cancel = default)
    {
        var now = DtOffsetNow;
        string seriesName = $"{now._ToYymmddInt(yearTwoDigits: true)}_{now._ToHhmmssInt():D6}_{testName}";

        ShuffledEndlessQueue<string> sampleVoiceFileNameShuffleQueue;


        List<int> randIntListAll = new();
        for (int i = 0; i <= 98; i++)
        {
            randIntListAll.Add(i);
        }
        ShuffledEndlessQueue<int> speakerIdShuffleQueueAll = new ShuffledEndlessQueue<int>(randIntListAll);

        List<int> randIntListTokutei = new();
        //randIntListTokutei.Add(8);
        //randIntListTokutei.Add(4);
        //randIntListTokutei.Add(4);
        //randIntListTokutei.Add(43);
        //randIntListTokutei.Add(43);
        //randIntListTokutei.Add(48);
        //randIntListTokutei.Add(58);
        //randIntListTokutei.Add(58);
        //randIntListTokutei.Add(58);
        //randIntListTokutei.Add(60);
        //randIntListTokutei.Add(60);
        //randIntListTokutei.Add(68);
        //randIntListTokutei.Add(90);
        //randIntListTokutei.Add(90);


        randIntListTokutei.Add(58);
        randIntListTokutei.Add(58);
        randIntListTokutei.Add(60);
        randIntListTokutei.Add(60);
        randIntListTokutei.Add(43);
        randIntListTokutei.Add(48);
        randIntListTokutei.Add(90);

        ShuffledEndlessQueue<int> speakerIdShuffleQueueTokutei = new ShuffledEndlessQueue<int>(randIntListTokutei);


        var randSampleVoiceFilesList = await Lfs.EnumDirectoryAsync(sampleVoiceWavDirName, false, wildcard: "*.wav", cancel: cancel);
        if (randSampleVoiceFilesList.Any())
        {
            sampleVoiceFileNameShuffleQueue = new ShuffledEndlessQueue<string>(randSampleVoiceFilesList.Select(x => x.FullPath));
        }
        else
        {
            throw new CoresLibException($"Directory '{sampleVoiceWavDirName}' has no music files.");
        }

        for (int i = 0; i < maxTryCount; i++)
        {
            try
            {
                var sampleVoicePath = sampleVoiceFileNameShuffleQueue.Dequeue();

                string thisText = srcText.Substring(Secure.RandSInt31() % (srcText.Length - textLengthOfRandomPart), textLengthOfRandomPart);

                int speakerIdToUse = speakerId;

                if (speakerIdToUse < 0)
                {
                    if (speakerIdToUse == -2)
                    {
                        int rand1 = Secure.RandSInt31() % 3;
                        if (rand1 != 0)
                        {
                            speakerIdToUse = speakerIdShuffleQueueTokutei.Dequeue();
                        }
                        else
                        {
                            speakerIdToUse = speakerIdShuffleQueueAll.Dequeue();
                        }
                    }
                    else
                    {
                        speakerIdToUse = speakerIdShuffleQueueAll.Dequeue();
                    }
                }

                if (speakerIdToUse < 0) speakerIdToUse = 58;

                string storyTitle = testName + "_" + i.ToString("D5");

                await ConvertTextToVoiceAsync(AiTaskTtsEngine.VoiceVox, thisText, sampleVoicePath, dstVoiceDirPath, tmpVoiceBoxDir, tmpVoiceWavDir, randIntListTokutei, diffusionSteps, seriesName, storyTitle, false, new int[] { 100 }, cancel);
            }
            catch (Exception ex)
            {
                ex._Error();
                await Task.Delay(1000);
            }
        }
    }

    public static async Task<List<int>> ReadSpeakerIDListAsync(string listFileName, CancellationToken cancel = default)
    {
        List<int> ret = new List<int>();
        var body = await Lfs.ReadStringFromFileAsync(listFileName, cancel: cancel);
        foreach (var line in body._GetLines(true, true, trim: true))
        {
            var tokens = line._Split(StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries, " ");
            if (tokens.Length >= 2 && tokens[0]._IsFilled())
            {
                int speakerId = tokens[0]._ToInt();
                int num = tokens[1]._ToInt();

                for (int i = 0; i < num; i++)
                {
                    ret.Add(speakerId);
                }
            }
        }

        return ret;
    }

    public async Task ConvertAllTextToVoiceAsync(string srcDirPath, AiVoiceSettingFactory settingsFactory, string dstVoiceDirPath, string tmpVoiceBoxDir, string tmpVoiceWavDir, int[]? speedPercentList = null,
        Func<Task>? finalizeProc = null,
        CancellationToken cancel = default)
    {
        var seriesDirList = await Lfs.EnumDirectoryAsync(srcDirPath, cancel: cancel);

        foreach (var seriesDir in seriesDirList.Where(x => x.IsDirectory && x.IsCurrentOrParentDirectory == false && x.Name.StartsWith("_") == false).OrderBy(x => x.Name, StrCmpi))
        {
            var settings = settingsFactory.GetAiVoiceSetting(seriesDir.FullPath);

            bool mixedMode = settings.MixedMode;

            ShuffledEndlessQueue<string>? sampleVoiceFileNameShuffleQueue = null;
            if (await Lfs.IsDirectoryExistsAsync(settings.SrcSampleVoiceFileNameOrRandDir, cancel))
            {
                var randSampleVoiceFilesList = await Lfs.EnumDirectoryAsync(settings.SrcSampleVoiceFileNameOrRandDir, false, wildcard: "*.wav", cancel: cancel);
                if (randSampleVoiceFilesList.Any())
                {
                    sampleVoiceFileNameShuffleQueue = new ShuffledEndlessQueue<string>(randSampleVoiceFilesList.Select(x => x.FullPath));
                }
                else
                {
                    throw new CoresLibException($"Directory '{settings.SrcSampleVoiceFileNameOrRandDir}' has no music files.");
                }
            }

            ShuffledEndlessQueue<int>? speakerIdShuffleForRotatin = null;

            List<int> speakerIdListInOneFile = new List<int>();

            if (settings.SpeakerIdStrOrListFilePath._IsNumber())
            {
                speakerIdListInOneFile.Add(settings.SpeakerIdStrOrListFilePath._ToInt());
                mixedMode = false;
            }
            else
            {
                if (mixedMode == false)
                {
                    speakerIdShuffleForRotatin = new ShuffledEndlessQueue<int>(await ReadSpeakerIDListAsync(settings.SpeakerIdStrOrListFilePath, cancel));
                }
                else
                {
                    speakerIdListInOneFile = await ReadSpeakerIDListAsync(settings.SpeakerIdStrOrListFilePath, cancel);
                }
            }

            var srcTextList = await Lfs.EnumDirectoryAsync(seriesDir.FullPath, false, cancel: cancel);

            foreach (var srcTextFile in srcTextList.Where(x => x.IsFile && x.Name._IsExtensionMatch(Consts.Extensions.Text) && x.Name.StartsWith("_") == false && x.Name.EndsWith(".ok.txt", StrCmpi) == false).OrderBy(x => x.Name, StrCmpi)
                ._Shuffle()
                .ToList())
            {
                try
                {
                    string srcSampleVoiceFile = settings.SrcSampleVoiceFileNameOrRandDir;

                    if (sampleVoiceFileNameShuffleQueue != null)
                    {
                        srcSampleVoiceFile = sampleVoiceFileNameShuffleQueue.Dequeue();
                    }

                    /*int speakerIdToUse = speakerId;

                    if (speakerIdToUse < 0)
                    {
                        speakerIdToUse = Secure.RandSInt31() % 99; /* 0 ～ 98 */
                    //}*/

                    string srcText = await Lfs.ReadStringFromFileAsync(srcTextFile.FullPath, maxSize: 2 * 1024 * 1024, cancel: cancel);

                    string seriesName = PP.GetFileName(PP.GetDirectoryName(srcTextFile.FullPath))._Normalize(false, true, false, true);
                    string storyTitle = PPWin.GetFileNameWithoutExtension(srcTextFile.FullPath)._Normalize(false, true, false, true);

                    List<int> speakerIdListForThisFile;

                    if (mixedMode == false)
                    {
                        speakerIdListForThisFile = speakerIdShuffleForRotatin!.Dequeue()._SingleList();
                    }
                    else
                    {
                        speakerIdListForThisFile = speakerIdListInOneFile.ToList();
                    }

                    if (settings.ReplaceStrList != null)
                    {
                        foreach (var kv in settings.ReplaceStrList)
                        {
                            srcText = srcText._ReplaceStr(kv.Key, kv.Value);
                        }
                    }

                    await ConvertTextToVoiceAsync(settings.TtsEngine, srcText, srcSampleVoiceFile, dstVoiceDirPath, tmpVoiceBoxDir, tmpVoiceWavDir, speakerIdListForThisFile, settings.DiffusionSteps, seriesName, storyTitle, settings.OverwriteSilent, speedPercentList, cancel);

                    // テキストファイルの先頭に _ を付ける
                    string newFilePath = PP.Combine(PP.GetDirectoryName(srcTextFile.FullPath), "_" + PP.GetFileName(srcTextFile.FullPath));

                    await Lfs.MoveFileAsync(srcTextFile.FullPath, newFilePath);

                    try
                    {
                        if (finalizeProc != null)
                        {
                            await finalizeProc();
                        }
                    }
                    catch (Exception ex)
                    {
                        srcTextFile.FullPath._Error();
                        ex._Error();
                    }
                }
                catch (Exception ex)
                {
                    srcTextFile.FullPath._Error();
                    ex._Error();
                }
            }
        }
    }

    public async Task VoiceChangeAsync(string srcPath, string srcSampleVoicePath, string? dstVoiceDirPath, string tmpVoiceWavDir, int diffusionSteps, string product, string series, string title, int track, int[]? speedPercentList = null, int headOnlySecs = 0, string? productAlternativeForTmpWavFiles = null, bool useOkFile = true, CancellationToken cancel = default)
    {
        if (speedPercentList == null || speedPercentList.Any() == false)
            speedPercentList = new int[] { 100 };

        if (productAlternativeForTmpWavFiles._IsEmpty()) productAlternativeForTmpWavFiles = product;

        string safeProduct = PPWin.MakeSafeFileName(product, true, true, true);
        string safeProductForTmpWav = PPWin.MakeSafeFileName(productAlternativeForTmpWavFiles, true, true, true);
        string safeSeries = PPWin.MakeSafeFileName(series, true, true, true);
        string safeTitle = PPWin.MakeSafeFileName(title, true, true, true);

        string safeVoiceTitle = PPWin.GetFileNameWithoutExtension(srcSampleVoicePath)._Normalize(false, true, false, true);

        await Lfs.CreateDirectoryAsync(tmpVoiceWavDir, cancel: cancel);

        if (dstVoiceDirPath._IsFilled())
        {
            await Lfs.CreateDirectoryAsync(dstVoiceDirPath, cancel: cancel);
        }

        string tmpVoiceWavPath = PP.Combine(tmpVoiceWavDir, $"{safeProductForTmpWav} - {safeSeries} - {safeTitle} - {safeVoiceTitle}.wav");

        string tagTitle = $"{safeProduct} - {safeSeries} - {safeTitle} - {safeVoiceTitle}";

        string srcTmpWavPath = await Lfs.GenerateUniqueTempFilePathAsync("seedvc_src", ".wav", cancel: cancel);

        string digest = $"voiceSamplePath={srcSampleVoicePath},diffusionSteps={diffusionSteps},targetMaxVolume={Settings.AdjustAudioTargetMaxVolume},targetMeanVolume={Settings.AdjustAudioTargetMeanVolume},headOnlySecs={headOnlySecs}";

        if (useOkFile == false || await Lfs.IsOkFileExistsAsync(tmpVoiceWavPath, digest, AiUtilVersion.CurrentVersion, cancel: cancel) == false)
        {
            await FfMpeg.EncodeAudioAsync(srcPath, srcTmpWavPath, FfMpegAudioCodec.Wav, tagTitle: tagTitle, headOnlySecs: headOnlySecs, cancel: cancel);

            await using (var seedvc = new AiUtilSeedVcEngine(this.Settings, this.FfMpeg))
            {
                await seedvc.ConvertAsync(srcTmpWavPath, tmpVoiceWavPath, srcSampleVoicePath, diffusionSteps, false, tagTitle, false, cancel: cancel);
                if (useOkFile) await Lfs.WriteOkFileAsync(tmpVoiceWavPath, new OkFileEmptyMetaData(), digest, AiUtilVersion.CurrentVersion, cancel);
                await Lfs.DeleteFileIfExistsAsync(srcTmpWavPath, cancel: cancel);
            }
        }

        if (dstVoiceDirPath._IsFilled())
        {
            string trackStr = track.ToString("D2");

            foreach (int speed in speedPercentList)
            {
                string speedStr = "x" + ((double)speed / 100.0).ToString(".00");

                MediaMetaData meta = new MediaMetaData
                {
                    Artist = $"{safeProduct} - {safeSeries} - {speedStr}",
                    Album = $"{safeProduct} - {speedStr}",
                    Title = $"{safeSeries} - [{trackStr}] {safeTitle} - {safeVoiceTitle} - {speedStr} - {safeProduct}",
                    TrackTotal = 999,
                    Track = track,
                };

                string dstVoiceFlacPath = PP.Combine(dstVoiceDirPath, safeProduct, safeSeries, $"{safeProduct} - {safeSeries} - {speedStr} - [{trackStr}] {safeTitle} - {safeVoiceTitle}.m4a");

                await FfMpeg.EncodeAudioAsync(tmpVoiceWavPath, dstVoiceFlacPath, FfMpegAudioCodec.Aac, 0, speed, meta, tagTitle, useOkFile, cancel: cancel);
            }
        }
    }

    public async Task ConvertTextToVoiceAsync(AiTaskTtsEngine engine, string srcText, string srcSampleVoicePath, string dstVoiceDirPath, string tmpVoiceBoxDir, string tmpVoiceWavDir, IEnumerable<int> speakerIdList, int diffusionSteps, string seriesName, string storyTitle, bool overwriteSilent, int[]? speedPercentList = null, CancellationToken cancel = default)
    {
        if (speedPercentList == null || speedPercentList.Any() == false)
            speedPercentList = new int[] { 100 };

        string speakerIdStr;
        if (speakerIdList.Count() == 1)
        {
            speakerIdStr = speakerIdList.Single().ToString("D3");
        }
        else
        {
            speakerIdStr = "mixed";
        }

        string safeSeriesName = PPWin.MakeSafeFileName(seriesName, true, true, true);

        string safeStoryTitle = PPWin.MakeSafeFileName(storyTitle, true, true, true);

        string safeVoiceTitle = PPWin.GetFileNameWithoutExtension(srcSampleVoicePath)._Normalize(false, true, false, true);

        string tmpVoiceBoxWavPath = PP.Combine(tmpVoiceBoxDir, $"{safeSeriesName} - {safeStoryTitle} - {speakerIdStr}.wav");

        string tagTitle = $"{safeSeriesName} - {safeStoryTitle} - {speakerIdStr}";

        FfMpegParsedList parsed;

        if (engine == AiTaskTtsEngine.VoiceVox)
        {
            await using (var vv = new AiUtilVoiceVoxEngine(this.Settings, this.FfMpeg))
            {
                if (tagTitle._IsEmpty()) tagTitle = storyTitle._TruncStrEx(16);

                parsed = await vv.TextToWavAsync(srcText, speakerIdList, tmpVoiceBoxWavPath, tagTitle, true, cancel);
            }
        }
        else if (engine == AiTaskTtsEngine.FishAudioPlain || engine == AiTaskTtsEngine.FishAudioWithSeedVc)
        {
            await using (var vv = new AiUtilFishAudioEngine(this.Settings, this.FfMpeg))
            {
                if (tagTitle._IsEmpty()) tagTitle = storyTitle._TruncStrEx(16);

                parsed = await vv.TextToWavAsync(srcText, speakerIdList, tmpVoiceBoxWavPath, tagTitle, true, cancel);
            }
        }
        else
        {
            throw new CoresLibException($"Unknown engine: {engine.ToString()}");
        }

        string tmpVoiceWavPath = PP.Combine(tmpVoiceWavDir, $"{safeSeriesName} - {safeStoryTitle} - {safeVoiceTitle} - {speakerIdStr}.wav");

        tagTitle = $"{safeSeriesName} - {safeStoryTitle} - {safeVoiceTitle} - {speakerIdStr}";

        AvUtilSeedVcMetaData vcMetaData;

        List<MediaVoiceSegment>? voiceSegments;

        if (engine != AiTaskTtsEngine.FishAudioPlain)
        {
            await using (var seedvc = new AiUtilSeedVcEngine(this.Settings, this.FfMpeg))
            {
                vcMetaData = await seedvc.ConvertAsync(tmpVoiceBoxWavPath, tmpVoiceWavPath, srcSampleVoicePath, diffusionSteps, overwriteSilent, tagTitle, true, parsed.Options_VoiceSegmentsList, cancel: cancel);

                voiceSegments = vcMetaData.VoiceSegments;
            }
        }
        else
        {
            voiceSegments = parsed.Options_VoiceSegmentsList;

            await Lfs.CopyFileAsync(tmpVoiceBoxWavPath, tmpVoiceWavPath);
        }

        foreach (int speed in speedPercentList)
        {
            string speedStr = "x" + ((double)speed / 100.0).ToString(".00");

            MediaMetaData meta = new MediaMetaData
            {
                Album = safeSeriesName + $" - {speedStr}",
                Title = $"{safeStoryTitle} - {safeVoiceTitle} - {speakerIdStr} - {speedStr}",
                Artist = $"{safeSeriesName} - {safeVoiceTitle} - {speedStr}",
            };

            string tailStr = "";
            if (speakerIdStr._IsDiffi("mixed"))
            {
                tailStr = " - " + speakerIdStr;
            }

            string dstVoiceFlacPath = PP.Combine(dstVoiceDirPath, safeSeriesName, $"{speedStr} - {safeStoryTitle} - {safeVoiceTitle}{tailStr}.flac");

            await FfMpeg.EncodeAudioAsync(tmpVoiceWavPath, dstVoiceFlacPath, FfMpegAudioCodec.Flac, 0, speed, meta, tagTitle, true, voiceSegments: voiceSegments, cancel: cancel);
        }
    }

    public async Task<FfMpegParsedList> ReplaceSongVoiceAsync(string srcMusicWavPath, string srcVocalWavPath, string sampleVoicePath, string dstWavPath, string tmpDir, int diffusionSteps, CancellationToken cancel = default)
    {
        string digest = $"x:{srcMusicWavPath}:{srcVocalWavPath}:{sampleVoicePath}:{dstWavPath}:{tmpDir}:{diffusionSteps}"._Digest();
        var okRead = await Lfs.ReadOkFileAsync<FfMpegParsedList>(dstWavPath, digest, AiUtilVersion.CurrentVersion, cancel: cancel);
        if (okRead.IsOk && okRead.Value != null) return okRead.Value;

        string normalizedVocalWavPath = PP.Combine(tmpDir, "adjusted_vocal_cache", PP.MakeSafeParentDirAndFilenameWithoutExtension(srcVocalWavPath)) + ".wav";
        string normalizedMusicWavPath = PP.Combine(tmpDir, "adjusted_music_cache", PP.MakeSafeParentDirAndFilenameWithoutExtension(srcMusicWavPath)) + ".wav";
        string changedVocalCachePath = PP.Combine(tmpDir, "changed_vocal_cache", PP.MakeSafeParentDirAndFilenameWithoutExtension(srcVocalWavPath) + "_to_" + PP.GetFileNameWithoutExtension(sampleVoicePath)) + ".wav";

        var srcVocalParsed = await FfMpeg.AdjustAudioVolumeAsync(
            srcVocalWavPath, normalizedVocalWavPath, CoresConfig.DefaultAiUtilSettings.AdjustAudioTargetMaxVolume, CoresConfig.DefaultAiUtilSettings.AdjustAudioTargetMeanVolume,
            FfmpegAdjustVolumeOptiono.MeanOnly, PP.GetFileNameWithoutExtension(srcVocalWavPath), true, cancel: cancel);

        var srcMusicParsed = await FfMpeg.AdjustAudioVolumeAsync(
            srcMusicWavPath, normalizedMusicWavPath, CoresConfig.DefaultAiUtilSettings.AdjustAudioTargetMaxVolume, CoresConfig.DefaultAiUtilSettings.AdjustAudioTargetMeanVolume,
            FfmpegAdjustVolumeOptiono.MeanOnly, PP.GetFileNameWithoutExtension(srcMusicWavPath), true, cancel: cancel);

        // vocal と music の dB 差分を計算
        double dbDelta = srcMusicParsed.Item1.VolumeDetect_MeanVolume - srcVocalParsed.Item1.VolumeDetect_MeanVolume;

        AvUtilSeedVcMetaData vcMetaData;

        await using (var seedvc = new AiUtilSeedVcEngine(this.Settings, this.FfMpeg, true))
        {
            vcMetaData = await seedvc.ConvertAsync(normalizedVocalWavPath, changedVocalCachePath, sampleVoicePath, diffusionSteps, false, PP.GetFileNameWithoutExtension(srcVocalWavPath), true, null, cancel: cancel);
        }

        double targetVocalVolume = srcMusicParsed.Item2.VolumeDetect_MeanVolume - dbDelta;

        string adjustedChangedVoiceWavTmpPath = await Lfs.GenerateUniqueTempFilePathAsync("adj", ".wav", cancel: cancel);

        await FfMpeg.AdjustAudioVolumeAsync(changedVocalCachePath, adjustedChangedVoiceWavTmpPath, 0, targetVocalVolume, FfmpegAdjustVolumeOptiono.MeanOnly,
            PP.GetFileNameWithoutExtension(srcVocalWavPath), true, cancel: cancel);

        string dstTmpWavPath = await Lfs.GenerateUniqueTempFilePathAsync("adj2", ".wav", cancel: cancel);

        await FfMpeg.AddBgmToVoiceFileAsync(adjustedChangedVoiceWavTmpPath, normalizedMusicWavPath, dstTmpWavPath, false, true, cancel: cancel);

        var result = await FfMpeg.AdjustAudioVolumeAsync(dstTmpWavPath, dstWavPath, CoresConfig.DefaultAiUtilSettings.AdjustAudioTargetMaxVolume, CoresConfig.DefaultAiUtilSettings.AdjustAudioTargetMeanVolume,
            FfmpegAdjustVolumeOptiono.MeanOnly, PP.GetFileNameWithoutExtension(srcMusicWavPath), true, cancel: cancel);

        await Lfs.WriteOkFileAsync(dstWavPath, result.Item2, digest, AiUtilVersion.CurrentVersion, cancel: cancel);

        return result.Item2;
    }

    public async Task EncodeAndNormalizeAllMusicWithSeqNoInputsAsync(string srcDirPath, string dstMusicDirPath, string tmpBaseDir, string albumName, CancellationToken cancel = default)
    {
        const int maxSongTitle = 32;

        albumName = albumName._RemoveQuotation('[', ']');

        albumName = PPWin.MakeSafeFileName(albumName.ToLowerInvariant(), true, true, true, true);

        albumName._NotEmptyCheck(nameof(albumName));

        string albumName2 = $"[{albumName}]";

        string albumName2ForExistsCheck = $"[{albumName}_";

        dstMusicDirPath._NotEmptyCheck(nameof(dstMusicDirPath));

        await Lfs.CreateDirectoryAsync(dstMusicDirPath, cancel: cancel);

        string tmpMusicDirPath = PP.Combine(tmpBaseDir, "0_MusicRelease_TMP2");

        var artistsDirList = await Lfs.EnumDirectoryAsync(srcDirPath, cancel: cancel);

        var srcAllMusicFilesList = (await Lfs.EnumDirectoryAsync(srcDirPath, recursive: true, cancel: cancel)).Where(x => x.IsFile && x.Name._IsExtensionMatch(Consts.Extensions.Filter_MusicFiles)).OrderBy(x => x.FullPath, StrCmpi)
            ._Shuffle()
            .ToList();

        foreach (var srcMusicFile in srcAllMusicFilesList)
        {
            var currentDstDirFiles = await Lfs.EnumDirectoryAsync(dstMusicDirPath, cancel: cancel);
            var currentDstMp3Files = currentDstDirFiles.Where(x => x.IsFile && x.Name._IsExtensionMatch(".mp3"));

            string srcMusicFileRelativePath = PP.GetRelativeFileName(srcMusicFile.FullPath, srcDirPath);
            string artistName = PP.SplitRelativePathToElements(srcMusicFileRelativePath)[0];

            string safeArtistName = PPWin.MakeSafeFileName(artistName, true, true, true, true);

            try
            {
                string songTitle = PPWin.GetFileNameWithoutExtension(srcMusicFile.Name)._NormalizeSoftEther(true);
                string safeSongTitle = PPWin.MakeSafeFileName(songTitle, true, true, true, true);

                string tmpOriginalSongWavPath = PP.Combine(tmpMusicDirPath, $"MusicRelease - {safeArtistName} - {safeSongTitle}.wav");

                var result = await EncodeAndNormalizeMusicAsync(srcMusicFile.FullPath, tmpOriginalSongWavPath, safeSongTitle, cancel: cancel);

                string formalSongTitle = (result?.Meta?.Title)._NonNullTrimSe();
                if (formalSongTitle._IsEmpty())
                {
                    formalSongTitle = songTitle;
                }

                formalSongTitle = PPWin.MakeSafeFileName(formalSongTitle, false, true, true, true);

                await Lfs.CreateDirectoryAsync(dstMusicDirPath, cancel: cancel);

                string formalSongTitleForFileName = $"{formalSongTitle._TruncStr(maxSongTitle)}.mp3";

                string existsCheckStr = $" - {safeArtistName} - {formalSongTitleForFileName}";

                string artistTag = $" - {safeArtistName} - ";

                var existsSameMP3 = currentDstMp3Files.Where(x => (x.Name.StartsWith(albumName2ForExistsCheck, StrCmpi) || x.Name.StartsWith("_" + albumName2ForExistsCheck, StrCmpi)) && x.Name.EndsWith(existsCheckStr, StrCmpi)).FirstOrDefault();

                if (existsSameMP3 != null)
                {
                    string fp2 = existsSameMP3.FullPath;

                    if (existsSameMP3.Name.StartsWith("_"))
                    {
                        fp2 = PP.Combine(PP.GetDirectoryName(fp2), existsSameMP3.Name.Substring(1));
                    }

                    if (await Lfs.IsOkFileExistsAsync(fp2, skipTargetFilePathCheck: true, cancel: cancel))
                    {
                        // すでに存在
                        continue;
                    }
                }

                int currentMaxNumber = 0;

                foreach (var currentMp3 in currentDstMp3Files.OrderByDescending(x => x.Name, StrCmpi))
                {
                    var tokens = currentMp3.Name._Split(StringSplitOptions.None, " - ");
                    if (tokens.Length >= 1)
                    {
                        string tmp1 = tokens[0];
                        if (tmp1.StartsWith("[") && tmp1.EndsWith("]"))
                        {
                            tmp1 = tmp1._RemoveQuotation('[', ']');

                            string startWithStr = albumName + "_";

                            if (tmp1.StartsWith(startWithStr, StrCmpi))
                            {
                                string numberParts = tmp1.Substring(startWithStr.Length);
                                int number = numberParts._ToInt();
                                if (number >= 1)
                                {
                                    if (currentMaxNumber < number)
                                    {
                                        if (await Lfs.IsOkFileExistsAsync(currentMp3.FullPath))
                                        {
                                            currentMaxNumber = number;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                currentMaxNumber++;

                string albumName2WithNumber = $"[{albumName}_{currentMaxNumber:D4}]";

                MediaMetaData meta = new MediaMetaData
                {
                    Album = albumName2,
                    Title = albumName2WithNumber + " - " + formalSongTitle,
                    Artist = albumName2 + " - " + artistName,
                };

                string dstMusicMp3Path = PP.Combine(dstMusicDirPath, $"{albumName2WithNumber} - {safeArtistName} - {formalSongTitle._TruncStr(maxSongTitle)}.mp3");

                await FfMpeg.EncodeAudioAsync(tmpOriginalSongWavPath, dstMusicMp3Path, FfMpegAudioCodec.Mp3, 320, 100, meta, safeSongTitle, cancel: cancel);
            }
            catch (Exception ex)
            {
                srcMusicFile.FullPath._Error();
                ex._Error();
            }
        }
    }


    public async Task EncodeVocalOnlyAllMusicWithSeqNoInputsAsync(string srcDirPath, string dstMusicDirPath, string albumName, CancellationToken cancel = default)
    {
        const int maxSongTitle = 32;

        albumName = albumName._RemoveQuotation('[', ']');

        albumName = PPWin.MakeSafeFileName(albumName.ToLowerInvariant(), true, true, true, true);

        albumName._NotEmptyCheck(nameof(albumName));

        string albumName2 = $"[{albumName}]";

        string albumName2ForExistsCheck = $"[{albumName}_";

        dstMusicDirPath._NotEmptyCheck(nameof(dstMusicDirPath));

        await Lfs.CreateDirectoryAsync(dstMusicDirPath, cancel: cancel);

        //string tmpMusicDirPath = PP.Combine(tmpBaseDir, "0_MusicRelease_TMP2");

        var artistsDirList = await Lfs.EnumDirectoryAsync(srcDirPath, cancel: cancel);

        var srcAllMusicFilesList = (await Lfs.EnumDirectoryAsync(srcDirPath, recursive: true, cancel: cancel)).Where(x => x.IsFile && x.Name._IsExtensionMatch(".wav")).OrderBy(x => x.FullPath, StrCmpi)
            ._Shuffle()
            .ToList();

        foreach (var srcMusicFile in srcAllMusicFilesList)
        {
            var currentDstDirFiles = await Lfs.EnumDirectoryAsync(dstMusicDirPath, cancel: cancel);
            var currentDstMp3Files = currentDstDirFiles.Where(x => x.IsFile && x.Name._IsExtensionMatch(".mp3"));

            //string srcMusicFileRelativePath = PP.GetRelativeFileName(srcMusicFile.FullPath, srcDirPath);
            //string artistName = PP.SplitRelativePathToElements(srcMusicFileRelativePath)[0];

            // VocalOnly - xxx - bbb.wav

            string fname = PP.GetFileNameWithoutExtension(srcMusicFile.FullPath, false);

            string[] tokens2 = fname._Split(StringSplitOptions.None, " - ");
            if (tokens2.Length < 3) continue;

            string artistName = tokens2[1].Trim();
            string musicName = tokens2[2].Trim();

            if (!(artistName._IsFilled() && musicName._IsFilled()))
            {
                continue;
            }

            string safeArtistName = PPWin.MakeSafeFileName(artistName, true, true, true, true);

            try
            {
                string songTitle = musicName;
                string safeSongTitle = PPWin.MakeSafeFileName(songTitle, true, true, true, true);

                //string tmpOriginalSongWavPath = PP.Combine(tmpMusicDirPath, $"MusicRelease - {safeArtistName} - {safeSongTitle}.wav");

                //var result = await EncodeAndNormalizeMusicAsync(srcMusicFile.FullPath, tmpOriginalSongWavPath, safeSongTitle, cancel: cancel);

                string formalSongTitle = songTitle;

                //formalSongTitle = PPWin.MakeSafeFileName(formalSongTitle, false, true, true, true);

                await Lfs.CreateDirectoryAsync(dstMusicDirPath, cancel: cancel);

                string formalSongTitleForFileName = $"{formalSongTitle._TruncStr(maxSongTitle)}.mp3";

                string existsCheckStr = $" - {safeArtistName} - {formalSongTitleForFileName}";

                string artistTag = $" - {safeArtistName} - ";

                var existsSameMP3 = currentDstMp3Files.Where(x => (x.Name.StartsWith(albumName2ForExistsCheck, StrCmpi) || x.Name.StartsWith("_" + albumName2ForExistsCheck, StrCmpi)) && x.Name.EndsWith(existsCheckStr, StrCmpi)).FirstOrDefault();

                if (existsSameMP3 != null)
                {
                    string fp2 = existsSameMP3.FullPath;

                    if (existsSameMP3.Name.StartsWith("_"))
                    {
                        fp2 = PP.Combine(PP.GetDirectoryName(fp2), existsSameMP3.Name.Substring(1));
                    }

                    if (await Lfs.IsOkFileExistsAsync(fp2, skipTargetFilePathCheck: true, cancel: cancel))
                    {
                        // すでに存在
                        continue;
                    }
                }

                int currentMaxNumber = 0;

                foreach (var currentMp3 in currentDstMp3Files.OrderByDescending(x => x.Name, StrCmpi))
                {
                    var tokens = currentMp3.Name._Split(StringSplitOptions.None, " - ");
                    if (tokens.Length >= 1)
                    {
                        string tmp1 = tokens[0];
                        if (tmp1.StartsWith("[") && tmp1.EndsWith("]"))
                        {
                            tmp1 = tmp1._RemoveQuotation('[', ']');

                            string startWithStr = albumName + "_";

                            if (tmp1.StartsWith(startWithStr, StrCmpi))
                            {
                                string numberParts = tmp1.Substring(startWithStr.Length);
                                int number = numberParts._ToInt();
                                if (number >= 1)
                                {
                                    if (currentMaxNumber < number)
                                    {
                                        if (await Lfs.IsOkFileExistsAsync(currentMp3.FullPath))
                                        {
                                            currentMaxNumber = number;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                currentMaxNumber++;

                string albumName2WithNumber = $"[{albumName}_{currentMaxNumber:D4}]";

                MediaMetaData meta = new MediaMetaData
                {
                    Album = albumName2,
                    Title = albumName2WithNumber + " - " + formalSongTitle,
                    Artist = albumName2 + " - " + artistName,
                };

                string dstMusicMp3Path = PP.Combine(dstMusicDirPath, $"{albumName2WithNumber} - {safeArtistName} - {formalSongTitle._TruncStr(maxSongTitle)}.mp3");

                await FfMpeg.EncodeAudioAsync(srcMusicFile.FullPath, dstMusicMp3Path, FfMpegAudioCodec.Mp3, 320, 100, meta, safeSongTitle, cancel: cancel);
            }
            catch (Exception ex)
            {
                srcMusicFile.FullPath._Error();
                ex._Error();
            }
        }
    }
    public async Task EncodeAndNormalizeAllMusicAsync(string srcDirPath, string dstMusicDirPath, string tmpBaseDir, string albumName, CancellationToken cancel = default)
    {
        string tmpMusicDirPath = PP.Combine(tmpBaseDir, "0_MusicRelease_TMP");

        var artistsDirList = await Lfs.EnumDirectoryAsync(srcDirPath, cancel: cancel);

        foreach (var artistDir in artistsDirList.Where(x => x.IsDirectory && x.IsCurrentOrParentDirectory == false).OrderBy(x => x.Name, StrCmpi))
        {
            string artistName = artistDir.Name._NormalizeSoftEther(true);
            string safeArtistName = PPWin.MakeSafeFileName(artistName, true, true, true);

            var srcMusicList = await Lfs.EnumDirectoryAsync(artistDir.FullPath, true, cancel: cancel);

            foreach (var srcMusicFile in srcMusicList.Where(x => x.IsFile && x.Name._IsExtensionMatch(Consts.Extensions.Filter_MusicFiles)).OrderBy(x => x.Name, StrCmpi)._Shuffle().ToList())
            {
                try
                {
                    string songTitle = PPWin.GetFileNameWithoutExtension(srcMusicFile.Name)._NormalizeSoftEther(true);
                    string safeSongTitle = PPWin.MakeSafeFileName(songTitle, true, true, true);

                    string tmpOriginalSongWavPath = PP.Combine(tmpMusicDirPath, $"MusicRelease - {safeArtistName} - {safeSongTitle}.wav");

                    var result = await EncodeAndNormalizeMusicAsync(srcMusicFile.FullPath, tmpOriginalSongWavPath, safeSongTitle, cancel: cancel);

                    if (dstMusicDirPath._IsFilled())
                    {
                        string formalSongTitle = (result?.Meta?.Title)._NonNullTrimSe();
                        if (formalSongTitle._IsEmpty())
                        {
                            formalSongTitle = songTitle;
                        }

                        formalSongTitle = PPWin.MakeSafeFileName(formalSongTitle, false, true, true);

                        await Lfs.CreateDirectoryAsync(dstMusicDirPath, cancel: cancel);

                        var currentDstDirFiles = await Lfs.EnumDirectoryAsync(dstMusicDirPath, cancel: cancel);
                        var currentDstAacFiles = currentDstDirFiles.Where(x => x.IsFile && x.Name._IsExtensionMatch(".m4a"));

                        string albumName2;

                        for (int i = 1; ; i++)
                        {
                            string tmp1 = albumName._RemoveQuotation('[', ']');
                            bool f1 = albumName.StartsWith('[') && albumName.EndsWith(']');
                            tmp1 += "_p" + i.ToString();
                            if (f1)
                            {
                                tmp1 = "[" + tmp1 + "]";
                            }

                            if (currentDstAacFiles.Where(x => x.Name.StartsWith(tmp1 + " - ", StrCmpi)).Count() < 1000) // 1 つの Album 名は最大 1000 件
                            {
                                albumName2 = tmp1;
                                break;
                            }
                        }

                        MediaMetaData meta = new MediaMetaData
                        {
                            Album = albumName2,
                            Title = formalSongTitle + " - " + albumName2,
                            Artist = albumName2 + " - " + artistName,
                        };

                        string dstMusicAacPath = PP.Combine(dstMusicDirPath, $"{albumName2} - {safeArtistName} - {formalSongTitle._TruncStr(48)}.m4a");

                        await FfMpeg.EncodeAudioAsync(tmpOriginalSongWavPath, dstMusicAacPath, FfMpegAudioCodec.Aac, 0, 100, meta, safeSongTitle, cancel: cancel);
                    }
                }
                catch (Exception ex)
                {
                    srcMusicFile.FullPath._Error();
                    ex._Error();
                }
            }
        }
    }

    public async Task<FfMpegParsedList> EncodeAndNormalizeMusicAsync(string srcFilePath, string dstWavPath, string tagTitle, bool useOkFile = true, CancellationToken cancel = default)
    {
        if (tagTitle._IsEmpty()) tagTitle = PP.GetFileNameWithoutExtension(srcFilePath);

        if (dstWavPath._IsEmpty()) throw new CoresLibException("dstWavPath is empty.");

        if (useOkFile)
        {
            var okFileCached = await Lfs.ReadOkFileAsync<FfMpegParsedList>(dstWavPath, "", AiUtilVersion.CurrentVersion, cancel: cancel);
            if (okFileCached.IsOk && okFileCached.Value != null)
            {
                return okFileCached.Value;
            }
        }

        // 音量調整
        string adjustedWavFile = await Lfs.GenerateUniqueTempFilePathAsync(srcFilePath, cancel: cancel);
        var result = await FfMpeg.AdjustAudioVolumeAsync(srcFilePath, adjustedWavFile, Settings.AdjustAudioTargetMaxVolume, Settings.AdjustAudioTargetMeanVolume, FfmpegAdjustVolumeOptiono.MeanOnly /* ! */, tagTitle, false, cancel);

        await Lfs.CopyFileAsync(adjustedWavFile, dstWavPath, param: new(flags: FileFlags.AutoCreateDirectory), cancel: cancel);

        if (useOkFile)
        {
            await Lfs.WriteOkFileAsync(dstWavPath, result.Item1, "", AiUtilVersion.CurrentVersion, cancel);
        }

        return result.Item1;
    }

    public async Task ExtractAllMusicAndVocalAsync(string srcDirPath, string dstMusicDirPath, string tmpBaseDir, string musicOnlyAlbumName, CancellationToken cancel = default)
    {
        string tmpMusicDirPath = PP.Combine(tmpBaseDir, "1_MusicOnly_TMP");
        string tmpVocalDirPath = PP.Combine(tmpBaseDir, "2_VocalOnly_TMP");

        var artistsDirList = await Lfs.EnumDirectoryAsync(srcDirPath, cancel: cancel);

        foreach (var artistDir in artistsDirList.Where(x => x.IsDirectory && x.IsCurrentOrParentDirectory == false).OrderBy(x => x.Name, StrCmpi))
        {
            string artistName = artistDir.Name._NormalizeSoftEther(true);
            string safeArtistName = PPWin.MakeSafeFileName(artistName, true, true, true);

            var srcMusicList = await Lfs.EnumDirectoryAsync(artistDir.FullPath, true, cancel: cancel);

            foreach (var srcMusicFile in srcMusicList.Where(x => x.IsFile && x.Name._IsExtensionMatch(Consts.Extensions.Filter_MusicFiles)).OrderBy(x => x.Name, StrCmpi)._Shuffle().ToList())
            {
                try
                {
                    string songTitle = PPWin.GetFileNameWithoutExtension(srcMusicFile.Name)._NormalizeSoftEther(true);
                    string safeSongTitle = PPWin.MakeSafeFileName(songTitle, true, true, true);

                    string tmpMusicWavPath = PP.Combine(tmpMusicDirPath, $"MusicOnly - {safeArtistName} - {safeSongTitle}.wav");
                    string tmpVocalWavPath = PP.Combine(tmpVocalDirPath, $"VocalOnly - {safeArtistName} - {safeSongTitle}.wav");

                    var result = await ExtractMusicAndVocalAsync(srcMusicFile.FullPath, tmpMusicWavPath, tmpVocalWavPath, safeSongTitle, cancel: cancel);

                    if (dstMusicDirPath._IsFilled())
                    {
                        string formalSongTitle = (result?.Meta?.Title)._NonNullTrimSe();
                        if (formalSongTitle._IsEmpty())
                        {
                            formalSongTitle = songTitle;
                        }

                        formalSongTitle = PPWin.MakeSafeFileName(formalSongTitle, false, true, true);

                        await Lfs.CreateDirectoryAsync(dstMusicDirPath, cancel: cancel);

                        var currentDstDirFiles = await Lfs.EnumDirectoryAsync(dstMusicDirPath, cancel: cancel);
                        var currentDstAacFiles = currentDstDirFiles.Where(x => x.IsFile && x.Name._IsExtensionMatch(".m4a"));

                        string musicOnlyAlbumName2;

                        for (int i = 1; ; i++)
                        {
                            string tmp1 = musicOnlyAlbumName._RemoveQuotation('[', ']');
                            bool f1 = musicOnlyAlbumName.StartsWith('[') && musicOnlyAlbumName.EndsWith(']');
                            tmp1 += "_p" + i.ToString();
                            if (f1)
                            {
                                tmp1 = "[" + tmp1 + "]";
                            }

                            if (currentDstAacFiles.Where(x => x.Name.StartsWith(tmp1 + " - ", StrCmpi)).Count() < 1000) // 1 つの Album 名は最大 1000 件
                            {
                                musicOnlyAlbumName2 = tmp1;
                                break;
                            }
                        }

                        MediaMetaData meta = new MediaMetaData
                        {
                            Album = musicOnlyAlbumName2,
                            Title = formalSongTitle + " - " + musicOnlyAlbumName2,
                            Artist = musicOnlyAlbumName2 + " - " + artistName,
                        };

                        string dstMusicAacPath = PP.Combine(dstMusicDirPath, $"{musicOnlyAlbumName2} - {safeArtistName} - {formalSongTitle._TruncStr(48)}.m4a");

                        await FfMpeg.EncodeAudioAsync(tmpMusicWavPath, dstMusicAacPath, FfMpegAudioCodec.Aac, 0, 100, meta, safeSongTitle, cancel: cancel);
                    }
                }
                catch (Exception ex)
                {
                    srcMusicFile.FullPath._Error();
                    ex._Error();
                }
            }
        }
    }


    public async Task<FfMpegParsedList> ExtractMusicAndVocalAsync(string srcFilePath, string dstMusicWavPath, string dstVocalWavPath, string tagTitle, bool useOkFile = true, CancellationToken cancel = default)
    {
        FfMpegParsedList ret = await TaskUtil.RetryAsync(async c =>
        {
            await using var uvr = new AiUtilUvrEngine(this.Settings, this.FfMpeg);

            return await uvr.ExtractAsync(srcFilePath, dstMusicWavPath, dstVocalWavPath, tagTitle, useOkFile, cancel);
        },
        1000,
        3,
        cancel,
        true);

        return ret;
    }

    public async Task<FfMpegParsedList> CompositAudioFileByAcxBcxTagsWithManyWavMaterialsAsync(string targetAudioFilePath, string dstAudioFilePath, FfMpegAudioCodec codec, int kbps,
        AiCompositWaveSettings settings,
        double targetSrcWavSpeed, MediaMetaData? metaData = null,
        CancellationToken cancel = default)
    {
        string digest = $"{targetAudioFilePath}:{metaData._ObjectToJson()}:{dstAudioFilePath}:{codec}:{targetSrcWavSpeed}"._Digest();

        var okCached = await Lfs.ReadOkFileAsync<FfMpegParsedList>(dstAudioFilePath, digest, AiUtilVersion.CurrentVersion, cancel: cancel);
        if (okCached.IsOk && okCached.Value != null)
        {
            return okCached.Value;
        }

        string tmpWavPath = await Lfs.GenerateUniqueTempFilePathAsync("a", "wav", cancel: cancel);

        await this.FfMpeg.EncodeAudioAsync(targetAudioFilePath, tmpWavPath, FfMpegAudioCodec.Wav, 0, useOkFile: false, cancel: cancel);

        var okRead = await Lfs.ReadOkFileAsync<FfMpegParsedList>(targetAudioFilePath, cancel: cancel);

        okRead.ThrowIfError();

        var usedFiles = await CompositWaveFileByAcxBcxTagsWithManyWavMaterialsAsync(tmpWavPath, okRead.Value._NullCheck(), tmpWavPath, settings, targetSrcWavSpeed, cancel);

        var parsed = await this.FfMpeg.EncodeAudioAsync(tmpWavPath, dstAudioFilePath, codec, kbps, metaData: metaData, useOkFile: false, cancel: cancel);

        parsed.Options_UsedBgmSrcMusicList = okRead.Value.Options_UsedBgmSrcMusicList._CloneDeep();
        parsed.Options_UsedOverwriteSrcMusicList = okRead.Value.Options_UsedOverwriteSrcMusicList._CloneDeep();
        parsed.Options_VoiceSegmentsList = okRead.Value.Options_VoiceSegmentsList._CloneDeep();
        parsed.Options_UsedMaterialsList = usedFiles.Options_UsedMaterialsList._CloneDeep();

        await Lfs.WriteOkFileAsync(dstAudioFilePath, parsed, digest, AiUtilVersion.CurrentVersion, cancel: cancel);

        return parsed;
    }

    public async Task<FfMpegParsedList> CompositWaveFileByAcxBcxTagsWithManyWavMaterialsAsync(string targetSrcWavPath, FfMpegParsedList targetSrcMetaData, string dstWavPath,
        AiCompositWaveSettings settings,
        double targetSrcWavSpeed,
        CancellationToken cancel = default)
    {
        FfMpegParsedList ret = targetSrcMetaData._CloneDeep();

        ret.Options_UsedMaterialsList = new();

        List<AiTaskOperationDesc> opList = new();

        var segments = targetSrcMetaData.Options_VoiceSegmentsList;
        segments._NullCheck();

        foreach (var rule in settings.RulesList)
        {
            for (int i = 0; i < segments.Count; i++)
            {
                var seg = segments[i];

                if (rule.EndTagStr._IsFilled())
                {
                    // 開始と終了の両方のタグがあるルール
                    if (seg.IsTag && rule.StartTagStr._Split(StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries, "|").Where(x => x._IsSamei(seg.TagStr)).Any())
                    {
                        for (int j = i + 1; j < segments.Count; j++)
                        {
                            var seg2 = segments[j];
                            if (seg2.IsTag && rule.EndTagStr._Split(StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries, "|").Where(x => x._IsSamei(seg2.TagStr)).Any())
                            {
                                if (rule.RuleData.MultipleMode_Count >= 2)
                                {
                                    for (int k = 0; k < rule.RuleData.MultipleMode_Count; k++)
                                    {
                                        double originalLen = seg2.TimePosition - seg.TimePosition;
                                        double thisLen = Math.Min(originalLen * Util.GenRandInterval(rule.RuleData.Multiplemode_SpanRatio._ToTimeSpanSecs()).TotalSeconds, originalLen);
                                        double thisStart = seg.TimePosition + (originalLen - thisLen) * Util.RandDouble0To1();

                                        opList.Add(new AiTaskOperationDesc { StartPosition = thisStart, EndPosition = thisStart + thisLen, Rule = rule, MatFilesQueue = rule.RuleData.MaterialsWavPathQueue });
                                    }
                                }
                                else
                                {
                                    opList.Add(new AiTaskOperationDesc { StartPosition = seg.TimePosition, EndPosition = seg2.TimePosition, Rule = rule, MatFilesQueue = rule.RuleData.MaterialsWavPathQueue });
                                }
                                break;
                            }
                        }
                    }
                }
                else
                {
                    // 開始タグしかないルール
                    if (seg.IsTag && rule.StartTagStr._Split(StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries, "|").Where(x => x._IsSamei(seg.TagStr)).Any())
                    {
                        var seg2 = seg._CloneDeep();
                        seg2.TimePosition = seg.TimePosition + Util.GenRandInterval(rule.RuleData.Param.DurationWhenEndTagIsEmpty._ToTimeSpanSecs(), 60).TotalSeconds;
                        if (rule.RuleData.MultipleMode_Count >= 2)
                        {
                            for (int k = 0; k < rule.RuleData.MultipleMode_Count; k++)
                            {
                                double originalLen = seg2.TimePosition - seg.TimePosition;
                                double thisLen = Math.Min(originalLen * Util.GenRandInterval(rule.RuleData.Multiplemode_SpanRatio._ToTimeSpanSecs()).TotalSeconds, originalLen);
                                double thisStart = seg.TimePosition + (originalLen - thisLen) * Util.RandDouble0To1();

                                opList.Add(new AiTaskOperationDesc { StartPosition = thisStart, EndPosition = thisStart + thisLen, Rule = rule, MatFilesQueue = rule.RuleData.MaterialsWavPathQueue });
                            }
                        }
                        else
                        {
                            opList.Add(new AiTaskOperationDesc { StartPosition = seg.TimePosition, EndPosition = seg2.TimePosition, Rule = rule, MatFilesQueue = rule.RuleData.MaterialsWavPathQueue });
                        }
                    }
                }
            }
        }

        HashSet<string> alreadyUsedList = new HashSet<string>(StrCmpi);

        foreach (var op in opList)
        {
            try
            {
                var param = op.Rule.RuleData.Param;

                double wantLength = op.EndPosition / targetSrcWavSpeed - op.StartPosition / targetSrcWavSpeed;
                double minLength = param.MarginSecs * 2 + wantLength + (param.StdFadeInSecs + param.StdFadeOutSecs) * 1.5 + param.MaxRandAfterLengthSecs + param.MaxRandBeforeLengthSecs + 3.0;

                string wavFilePath = "";
                double wavFileLength = 0.0;

                for (int j = 0; j < 10000; j++)
                {
                    wavFilePath = op.MatFilesQueue.Dequeue();
                    wavFileLength = await GetWavFileLengthSecsAsync(wavFilePath, cancel: cancel);
                    if (wavFileLength >= minLength)
                    {
                        break;
                    }
                    wavFilePath = "";
                }

                if (wavFilePath._IsEmpty())
                {
                    // もう候補なし
                    break;
                }

                MediaUsedMaterialsSegment used = new MediaUsedMaterialsSegment
                {
                    WavPath = wavFilePath,
                    LengthSecs = wantLength,
                    StartSecs = op.StartPosition,
                };

                if (settings.CreateAudioEffectFilter != null)
                {
                    var filter = settings.CreateAudioEffectFilter(op, cancel);
                    if (filter != null)
                    {
                        op.Filter = filter;

                        used.FilterName = filter.FilterName;
                        used.FilterSettings = filter.EffectSettings._ToJObject();
                        used.FilterSpeedType = filter.FilterSpeedType;
                    }
                }

                ret.Options_UsedMaterialsList.Add(used);

                double before_length = param.MaxRandBeforeLengthSecs * Util.RandDouble0To1();
                double after_length = param.MaxRandAfterLengthSecs * Util.RandDouble0To1();

                double len = wantLength + before_length + after_length;

                double matStartPos = Util.RandDouble0To1() * (wavFileLength - len - param.MarginSecs * 2) + param.MarginSecs;

                double fadeIn = Util.GenRandInterval(param.StdFadeInSecs._ToTimeSpanSecs()).TotalSeconds;
                double fadeOut = Util.GenRandInterval(param.StdFadeOutSecs._ToTimeSpanSecs()).TotalSeconds;

                double targetStartPos = Math.Max(op.StartPosition / targetSrcWavSpeed - before_length, 0.0);

                op.Calced_MaterialWavPath = wavFilePath;
                op.Calced_TargetPositionSecs = targetStartPos;
                op.Calced_MeterialPositionSecs = matStartPos;
                op.Calced_LengthSecs = len;
                op.Calced_FadeInSecs = fadeIn;
                op.Calced_FadeOutSecs = fadeOut;

                double randomValueLeft = (Util.RandDouble0To1() - 0.5) * 2 * param.VolumeDeltaRandomRange;
                double randomValueRight = (Util.RandDouble0To1() - 0.5) * 2 * param.VolumeDeltaRandomRange;

                op.Calced_VolumeDelta_Left = param.VolumeDelta + randomValueLeft;
                op.Calced_VolumeDelta_Right = param.VolumeDelta + randomValueRight;
            }
            catch (Exception ex)
            {
                ex._Error();
            }
        }

        await CompositWaveWithFadeAsync(targetSrcWavPath, dstWavPath, opList, cancel: cancel);

        return ret;
    }

    public async Task<double> GetWavFileLengthSecsAsync(string wavPath, CancellationToken cancel = default)
    {
        await using (var targetSrcFileStream = File.Open(wavPath, FileMode.Open, FileAccess.Read, FileShare.Read))
        {
            await using var reader = new WaveFileReader(targetSrcFileStream);

            CheckWavFormat(reader.WaveFormat);

            return reader.TotalTime.TotalSeconds;
        }
    }

    public static void CheckWavFormat(WaveFormat format)
    {
        if (format.SampleRate != 44100 ||
                      format.Channels != 2 ||
                      format.BitsPerSample != 16 ||
                      format.Encoding != WaveFormatEncoding.Pcm)
        {
            throw new CoresLibException(
                $"指定されたファイルは " +
                $"44.1 kHz / 2 チャンネル / 16 ビット / 無圧縮 PCM ではありません。"
            );
        }
    }

    // By ChatGPT
    /// <summary>
    /// targetWavRandomAccess の position 秒目から sourceWav の長さ分を差し替えます（PCM部のみ）。
    /// 差し替え区間の頭を fadeIn 秒かけてフェードイン（元音 → 新音へクロスフェード）、
    /// 尾を fadeOut 秒かけてフェードアウト（新音 → 元音へクロスフェード）します。
    ///
    /// 大容量（数十GB等）を想定し、ターゲットは IRandomAccess<byte> でランダムアクセスします。
    /// 高速化のため、対象区間を ReadRandomAsync 1回で読み出して Span<byte> 上で処理し、
    /// WriteRandomAsync 1回で書き戻します（Read/Write はそれぞれ合計1回のみ）。
    /// </summary>
    /// <param name="targetWavRandomAccess">差し替え対象のPCM（ヘッダ除く）を保持するランダムアクセス</param>
    /// <param name="sourceWav">挿入するPCMバイト列（ヘッダ除く、最大でも int に収まる想定）</param>
    /// <param name="position">差し替え開始位置（秒）</param>
    /// <param name="fadeIn">頭のクロスフェード時間（秒）</param>
    /// <param name="fadeOut">尾のクロスフェード時間（秒）</param>
    public static void ReplaceWavDataWithFadeInOut(
        IRandomAccess<byte> targetWavRandomAccess,
        ReadOnlyMemory<byte> sourceWav,
        double position,
        double fadeIn,
        double fadeOut)
    {
        // 固定前提（元実装と同じ）：44.1kHz / 16bit / Stereo / PCM（ヘッダ除くPCM部のみ）
        const int SampleRate = 44100;
        const int Channels = 2;
        const int BytesPerSample = 2;                 // 16bit
        const int BlockAlign = Channels * BytesPerSample; // 4 bytes / frame (L+R)

        if (targetWavRandomAccess is null) throw new ArgumentNullException(nameof(targetWavRandomAccess));
        if (position < 0) throw new ArgumentOutOfRangeException(nameof(position), "position は 0 以上である必要があります。");
        if (fadeIn < 0) throw new ArgumentOutOfRangeException(nameof(fadeIn), "fadeIn は 0 以上である必要があります。");
        if (fadeOut < 0) throw new ArgumentOutOfRangeException(nameof(fadeOut), "fadeOut は 0 以上である必要があります。");

        ReadOnlySpan<byte> sourceSpan = sourceWav.Span;

        // sourceWav は「PCM部のみ」前提。フレーム境界でない場合は処理不能。
        if (sourceSpan.Length % BlockAlign != 0)
            throw new ArgumentException($"sourceWav の長さが BlockAlign({BlockAlign}) の倍数ではありません。", nameof(sourceWav));

        int totalSourceFrames = sourceSpan.Length / BlockAlign;
        if (totalSourceFrames == 0)
        {
            // 差し替えるデータが無いなら何もしない（例外にせず終了）
            return;
        }

        // ターゲットは巨大になり得るため、ファイルサイズは long で扱う。
        // 指示：GetFileSizeAsync の refresh 引数は常に false。
        long targetFileSizeBytes = targetWavRandomAccess.GetFileSizeAsync(refresh: false, cancel: CancellationToken.None)
            .GetAwaiter().GetResult();

        if (targetFileSizeBytes < 0)
            throw new IOException("targetWavRandomAccess のファイルサイズが不正です。");

        // ターゲットもPCM部のみ前提。端数がある場合は最後の端数は無視（元実装の / と同じ切り捨て）。
        long totalTargetFrames = targetFileSizeBytes / BlockAlign;
        if (totalTargetFrames <= 0)
        {
            // ターゲットが空なら何もしない
            return;
        }

        // 開始フレーム（ターゲット上）とフェード長（フレーム単位）
        long posFrame = (long)Math.Round(position * SampleRate);
        int fadeInFrames = (int)Math.Round(fadeIn * SampleRate);
        int fadeOutFrames = (int)Math.Round(fadeOut * SampleRate);

        // 差し替えの理論上の末尾（ターゲット上）
        long endFrame = posFrame + totalSourceFrames;

        // ターゲット範囲外に出ないようにクリップ（元実装と同様）
        long startFrame = Math.Max(0L, posFrame);
        long clippedEndFrame = Math.Min(totalTargetFrames, endFrame);

        if (clippedEndFrame <= startFrame)
        {
            // 置換区間がターゲットに一切かからない
            return;
        }

        // 今回の処理対象フレーム数（これは sourceWav 長以下なので int に収まる想定）
        long segmentFramesLong = clippedEndFrame - startFrame;
        long segmentBytesLong = checked(segmentFramesLong * BlockAlign);

        if (segmentBytesLong > int.MaxValue)
        {
            // 指示の前提：sourceWav は最大でも約1.5GB程度で int に入る。
            // それでも安全のためガード。
            throw new IOException("処理対象区間が大きすぎてメモリに確保できません（int.MaxValue超）。");
        }

        int segmentBytes = (int)segmentBytesLong;
        int segmentFrames = (int)segmentFramesLong;

        // 1) ターゲットから「影響範囲」だけを 1回で読み込む
        byte[] segmentBuffer = new byte[segmentBytes];
        int readBytes = targetWavRandomAccess
            .ReadRandomAsync(
                position: checked(startFrame * BlockAlign),
                data: segmentBuffer,
                cancel: CancellationToken.None)
            .GetAwaiter().GetResult();

        // 指示：ReadRandomAsync の読み取り成功バイト数が意図と異なる場合は例外
        if (readBytes != segmentBytes)
            throw new IOException($"ReadRandomAsync の読み取りバイト数が不正です。期待={segmentBytes}, 実際={readBytes}");

        Span<byte> targetSegmentSpan = segmentBuffer.AsSpan();

        // 2) Span<byte> 上で高速にフレームループ（Read/Write をループ内で呼ばない）
        for (int localFrame = 0; localFrame < segmentFrames; localFrame++)
        {
            // ターゲット上の絶対フレーム番号
            long absoluteFrame = startFrame + localFrame;

            // source 上のフレーム番号（元実装：srcFrame = frame - posFrame）
            long srcFrameLong = absoluteFrame - posFrame;

            if ((ulong)srcFrameLong >= (ulong)totalSourceFrames)
                continue; // 念のため（理論上は start/end でほぼ範囲内）

            int srcFrame = (int)srcFrameLong;

            // ゲイン計算（0..1）
            double gain;
            if (fadeInFrames > 0 && srcFrame < fadeInFrames)
            {
                gain = (double)srcFrame / fadeInFrames;
            }
            else if (fadeOutFrames > 0 && srcFrame >= totalSourceFrames - fadeOutFrames)
            {
                gain = (double)(totalSourceFrames - srcFrame) / fadeOutFrames;
            }
            else
            {
                gain = 1.0;
            }

            // 0..1 にクランプ（元実装踏襲）
            if (gain < 0.0) gain = 0.0;
            if (gain > 1.0) gain = 1.0;

            int targetFrameOffset = localFrame * BlockAlign;
            int sourceFrameOffset = srcFrame * BlockAlign;

            // 各チャンネル（L/R）を 16bit little-endian として処理
            for (int ch = 0; ch < Channels; ch++)
            {
                int targetOffset = targetFrameOffset + ch * BytesPerSample;
                int sourceOffset = sourceFrameOffset + ch * BytesPerSample;

                // target（元音）サンプル取得
                short originalSample = (short)(
                    targetSegmentSpan[targetOffset] |
                    (targetSegmentSpan[targetOffset + 1] << 8));

                // source（新音）サンプル取得
                short sourceSample = (short)(
                    sourceSpan[sourceOffset] |
                    (sourceSpan[sourceOffset + 1] << 8));

                // クロスフェード：src*gain + orig*(1-gain)
                double mixed = sourceSample * gain + originalSample * (1.0 - gain);

                int rounded = (int)Math.Round(mixed);
                if (rounded > short.MaxValue) rounded = short.MaxValue;
                if (rounded < short.MinValue) rounded = short.MinValue;

                short resultSample = (short)rounded;

                // 書き戻し（little-endian）
                targetSegmentSpan[targetOffset] = (byte)(resultSample & 0xFF);
                targetSegmentSpan[targetOffset + 1] = (byte)((resultSample >> 8) & 0xFF);
            }
        }

        // 3) 更新した範囲を 1回で書き戻す
        targetWavRandomAccess
            .WriteRandomAsync(
                position: checked(startFrame * BlockAlign),
                data: segmentBuffer,
                cancel: CancellationToken.None)
            .GetAwaiter().GetResult();
    }



    /// <summary>
    /// 指定された時刻 currentTimeSec におけるフェードイン・フェードアウトの総合ゲイン値を計算する
    /// </summary>
    private static float ComputeFadeFactor(
        double currentTimeSec,
        double fadeInStartSec, double fadeInEndSec,
        double fadeOutStartSec, double fadeOutEndSec)
    {
        // フェードイン係数
        float fadeInFactor = 1.0f;
        // フェードイン区間が実質的にある場合のみ計算
        if (fadeInEndSec > fadeInStartSec)
        {
            if (currentTimeSec < fadeInStartSec)
            {
                // フェードイン開始前はゲイン 0
                fadeInFactor = 0.0f;
            }
            else if (currentTimeSec > fadeInEndSec)
            {
                // フェードイン完了後はゲイン 1
                fadeInFactor = 1.0f;
            }
            else
            {
                // その間は線形に 0->1 へ変化
                double length = fadeInEndSec - fadeInStartSec;
                fadeInFactor = (float)((currentTimeSec - fadeInStartSec) / length);
            }
        }

        // フェードアウト係数
        float fadeOutFactor = 1.0f;
        // フェードアウト区間が実質的にある場合のみ計算
        if (fadeOutEndSec > fadeOutStartSec)
        {
            if (currentTimeSec < fadeOutStartSec)
            {
                // フェードアウト開始前はゲイン 1
                fadeOutFactor = 1.0f;
            }
            else if (currentTimeSec > fadeOutEndSec)
            {
                // フェードアウト完了後はゲイン 0
                fadeOutFactor = 0.0f;
            }
            else
            {
                // その間は線形に 1->0 へ変化
                double length = fadeOutEndSec - fadeOutStartSec;
                double ratio = (currentTimeSec - fadeOutStartSec) / length;
                fadeOutFactor = (float)(1.0 - ratio);
            }
        }

        // 最終ゲインはフェードインとフェードアウトを掛け合わせる
        float amp = fadeInFactor * fadeOutFactor;

        // 安全のためクリップ（本来は 0～1 に収まるはず）
        if (amp < 0.0f) amp = 0.0f;
        if (amp > 1.0f) amp = 1.0f;

        return amp;
    }

    async Task CompositWaveWithFadeAsync(string targetSrcWavPath, string dstWavPath,
        IEnumerable<AiTaskOperationDesc> operations, CancellationToken cancel = default)
    {
        HugeMemoryBuffer<byte> targetSrcData2;
        //Memory<byte> targetSrcData;
        WaveFormat targetSrcWaveFormat;

        await using (var targetSrcFileStream = File.Open(targetSrcWavPath, FileMode.Open, FileAccess.Read, FileShare.Read))
        {
            await using var targetSrcReader = new WaveFileReader(targetSrcFileStream);
            targetSrcWaveFormat = targetSrcReader.WaveFormat;

            CheckWavFormat(targetSrcWaveFormat);

            targetSrcData2 = await targetSrcReader._ReadToEndHugeAsync(cancel: cancel);
            //targetSrcData = targetSrcReader._ReadToEnd().AsMemory();
        }

        foreach (var op in operations)
        {
            if (op.Calced_MaterialWavPath._IsFilled())
            {
                cancel.ThrowIfCancellationRequested();

                try
                {
                    Memory<byte> matSrcData;

                    //Console.WriteLine(op.MeterialWavPath);
                    await using (var matFileStream = File.Open(op.Calced_MaterialWavPath, FileMode.Open, FileAccess.Read, FileShare.Read))
                    {
                        await using var matReader = new WaveFileReader(matFileStream);
                        WaveFormat matWaveFormat = matReader.WaveFormat;

                        CheckWavFormat(targetSrcWaveFormat);

                        // 波形フォーマット前提値
                        const int sampleRate = 44100;
                        const int channels = 2;
                        const int bitsPerSample = 16;
                        // 1サンプル(1ch)あたりのバイト数
                        const int bytesPerSample = bitsPerSample / 8; // = 2
                                                                      // 1フレーム(全ch合計)あたりのバイト数(2ch前提)
                        const int blockAlign = channels * bytesPerSample; // 4バイト
                                                                          // 合成を開始するフレーム位置(整数)
                        long sourceStartFrame = (long)(op.Calced_MeterialPositionSecs * sampleRate);

                        // 合成するフレーム数(整数)
                        long framesToProcess = (long)(op.Calced_LengthSecs * sampleRate);
                        if (framesToProcess <= 0)
                        {
                            return;
                        }

                        // バイト単位での開始位置
                        long sourceStartByte = sourceStartFrame * blockAlign;
                        int bytesToProcess = (int)(framesToProcess * blockAlign);

                        matReader.Seek(sourceStartByte, SeekOrigin.Begin);

                        matSrcData = await matReader._ReadAllAsync(bytesToProcess, cancel: cancel);

                        if (op.Filter != null)
                        {
                            // フィルタ適用
                            //Where();
                            op.Filter.PerformFilterFunc(matSrcData, cancel);
                        }
                    }

                    AiWaveMixUtil.MixWaveData(targetSrcData2, matSrcData, op.Calced_TargetPositionSecs, 0, op.Calced_LengthSecs, op.Calced_VolumeDelta_Left, op.Calced_VolumeDelta_Right, op.Calced_FadeInSecs, op.Calced_FadeOutSecs);
                }
                catch (Exception ex)
                {
                    ex._Error();
                }
            }
        }

        cancel.ThrowIfCancellationRequested();

        await Lfs.CreateDirectoryAsync(PP.GetDirectoryName(dstWavPath), cancel: cancel);

        await using (var dstFileStream = File.Create(dstWavPath))
        {
            await using var writer = new WaveFileWriter(dstFileStream, targetSrcWaveFormat);

            await writer.FlushAsync(cancel);

            //await writer.WriteAsync(targetSrcData, cancel);

            await Util.CopyHugeMemoryBufferToStreamAsync(dstFileStream, targetSrcData2, cancel);
        }
    }

    public async Task AddRandomBgmToAllVoiceFilesAsync(string srcVoiceDirRoot, string dstDirRoot, FfMpegAudioCodec codec, AiRandomBgmSettingsFactory settingsFactory, int kbps = 0, string? oldTagStr = null, string? newTagStr = null, object? userParam = null, bool boostWithLength = false, CancellationToken cancel = default)
    {
        var srcFiles = await Lfs.EnumDirectoryAsync(srcVoiceDirRoot, true, cancel: cancel);

        foreach (var srcFile in srcFiles.Where(x => x.IsFile && x.Name._IsExtensionMatch(Consts.Extensions.Filter_MusicFiles) && x.Name._InStri("bgm") == false && PP.GetFileName(PP.GetDirectoryName(x.FullPath)).StartsWith("_") == false)
            .OrderBy(x => x.FullPath, StrCmpi)._Shuffle().ToList())
        {
            string relativeDirPth = PP.GetRelativeDirectoryName(PP.GetDirectoryName(srcFile.FullPath), srcVoiceDirRoot);

            string dstDirPath = PP.Combine(dstDirRoot, relativeDirPth);

            var settingsList = settingsFactory.GetBgmSettingListProc(srcFile.FullPath, userParam);

            HashSet<string> keyNames = new HashSet<string>(StrCmpi);
            settingsList._DoForEach(x => keyNames.Add(x.Key));

            foreach (var key in keyNames.OrderBy(x => x, StrCmpi))
            {
                Con.WriteLine($"Add BGM ({key}): '{srcFile.FullPath}' -> '{dstDirPath}'");

                var bgmSettings = settingsList.Where(x => x.Key._IsSamei(key)).First();

                var result = await AddRandomBgmToVoiceFileAsync(srcFile.FullPath, dstDirPath, codec, bgmSettings, kbps, true, oldTagStr, newTagStr, boostWithLength, cancel: cancel);
            }
        }
    }

    public async Task<(FfMpegParsedList Parsed, string DestFileName)> AddRandomBgmToVoiceFileAsync(string srcVoiceFilePath, string dstDir, FfMpegAudioCodec codec, KeyValuePair<string, AiRandomBgmSettings> settings, int kbps = 0, bool useOkFile = true, string? oldTagStr = null, string? newTagStr = null, bool boostWithLength = false, CancellationToken cancel = default)
    {
        List<AiRandomBgpReplaceRanges> replaceRanges = new();

        var srcVoiceFileMetaData = await FfMpeg.ReadMetaDataWithFfProbeAsync(srcVoiceFilePath, cancel: cancel);

        srcVoiceFileMetaData.ReParseMain();

        int srcDurationMsecs = srcVoiceFileMetaData.Input?.GetDurationMsecs() ?? -1;

        if (srcDurationMsecs < 0)
        {
            throw new CoresLibException($"Failed to get duration '{srcVoiceFilePath}'");
        }

        MediaMetaData srcMeta = srcVoiceFileMetaData.Meta;
        if (srcMeta == null) srcMeta = new MediaMetaData();

        var okFileMeta = await Lfs.ReadOkFileAsync<FfMpegParsedList>(srcVoiceFilePath, cancel: cancel);

        if (okFileMeta.IsOk && okFileMeta.Value != null && okFileMeta.Value.Meta != null)
        {
            okFileMeta.Value.ReParseMain();

            if (okFileMeta.Value.Meta.HasValue())
            {
                srcMeta = okFileMeta.Value.Meta;
            }

            var voiceSegList = okFileMeta.Value?.Options_VoiceSegmentsList;
            if (voiceSegList != null && voiceSegList.Count >= 1 && settings.Value.ReplaceBgmDirPath._IsFilled())
            {
                int currentLevel = 0;

                for (int i = 0; i < voiceSegList.Count; i++)
                {
                    var seg1 = voiceSegList[i];

                    if (seg1.TagStr._IsSamei("<ACX_START>"))
                    {
                        currentLevel += 1;
                    }
                    else if (seg1.TagStr._IsSamei("<ACX_END>"))
                    {
                        currentLevel -= 1;
                    }
                    else if (seg1.TagStr._IsSamei("<BCX_START>"))
                    {
                        currentLevel += 2;
                    }
                    else if (seg1.TagStr._IsSamei("<BCX_FINAL_START>"))
                    {
                        currentLevel += 2;
                    }
                    else if (seg1.TagStr._IsSamei("<XCSSTART>"))
                    {
                        currentLevel += 1;
                    }
                    else if (seg1.TagStr._IsSamei("<XCSEND>"))
                    {
                        currentLevel -= 1;
                    }
                    else if (seg1.TagStr._IsSamei("<BCX_END>") || seg1.TagStr._IsSamei("<BXC_END>"))
                    {
                        currentLevel -= 2;
                    }
                    else if (seg1.TagStr._IsSamei("<BCX_FINAL_END>") || seg1.TagStr._IsSamei("<BXC_FINAL_END>"))
                    {
                        currentLevel -= 2;
                    }

                    seg1.Level = currentLevel;

                    if (seg1.TagStr._IsSamei("<BCX_START>"))
                    {
                        for (int j = i; j < voiceSegList.Count; j++)
                        {
                            var seg2 = voiceSegList[j];
                            if (seg2.TagStr._IsSamei("<BCX_END>") || seg2.TagStr._IsSamei("<BXC_END>"))
                            {
                                replaceRanges.Add(new AiRandomBgpReplaceRanges
                                {
                                    Margin = 30,
                                    Length = seg2.TimePosition - seg1.TimePosition,
                                    FadeIn = 4,
                                    FadeOut = 4,
                                    StartPosition = seg1.TimePosition - 4,
                                });
                                break;
                            }
                        }
                    }
                    if (seg1.TagStr._IsSamei("<BCX_FINAL_START>"))
                    {
                        for (int j = i; j < voiceSegList.Count; j++)
                        {
                            var seg2 = voiceSegList[j];
                            if (seg2.TagStr._IsSamei("<BCX_FINAL_END>") || seg2.TagStr._IsSamei("<BXC_FINAL_END>"))
                            {
                                replaceRanges.Add(new AiRandomBgpReplaceRanges
                                {
                                    Margin = 30,
                                    Length = seg2.TimePosition - seg1.TimePosition,
                                    FadeIn = 4,
                                    FadeOut = 4,
                                    StartPosition = seg1.TimePosition - 4,
                                });
                                break;
                            }
                        }
                    }
                }
            }
        }

        MediaMetaData newMeta = srcMeta._CloneDeep();

        string newTagStrAddForKey = "";
        if (settings.Key._IsFilled())
        {
            newTagStrAddForKey += "_" + settings.Key;
        }

        if (oldTagStr._IsFilled() && newTagStr._IsFilled())
        {

            newTagStr += newTagStrAddForKey;

            newMeta.Album = newMeta.Album._ReplaceStr(oldTagStr, newTagStr);
            newMeta.AlbumArtist = newMeta.AlbumArtist._ReplaceStr(oldTagStr, newTagStr);
            newMeta.Title = newMeta.Title._ReplaceStr(oldTagStr, newTagStr);
            newMeta.Artist = newMeta.Artist._ReplaceStr(oldTagStr, newTagStr);
        }
        else
        {
            string bgm_x_replace_dst_str = $" - bgm{newTagStrAddForKey}_x";
            newMeta.Album = newMeta.Album._ReplaceStr(" - x", bgm_x_replace_dst_str);
            newMeta.AlbumArtist = newMeta.AlbumArtist._ReplaceStr(" - x", bgm_x_replace_dst_str);
            newMeta.Title = newMeta.Title._ReplaceStr(" - x", bgm_x_replace_dst_str);
            newMeta.Artist = newMeta.Artist._ReplaceStr(" - x", bgm_x_replace_dst_str);
        }

        string dstFileName;
        string dstExtension = FfMpegUtil.GetExtensionFromCodec(codec);

        if (oldTagStr._IsFilled() && newTagStr._IsFilled())
        {
            dstFileName = PP.GetFileNameWithoutExtension(srcVoiceFilePath)._ReplaceStr(oldTagStr, newTagStr) + dstExtension;
        }
        else
        {
            string tmp1 = PP.GetFileNameWithoutExtension(srcVoiceFilePath);
            string tmp2 = $"bgm{newTagStrAddForKey}_" + tmp1;

            dstFileName = tmp2 + dstExtension;
        }

        string dstFilePath = PP.Combine(dstDir, dstFileName);

        if (dstFilePath._IsSamei(srcVoiceFilePath))
        {
            throw new CoresLibException($"dstFilePath == srcVoiceFilePath: '{srcVoiceFilePath}'");
        }

        double adjustDelta = settings.Value.SmoothMode ? settings.Value.BgmVolumeDeltaForSmooth : settings.Value.BgmVolumeDeltaForConstant;

        string digest = $"srcDurationMsecs={srcDurationMsecs},fadeOutSecs={settings.Value.TailFadeoutSecs},smoothMode={settings.Value.SmoothMode},adjustDelta ={adjustDelta},codec={codec},kbps={kbps},meta={newMeta._ObjectToJson()._Digest()}";

        if (useOkFile)
        {
            var okParsed = await Lfs.ReadOkFileAsync<FfMpegParsedList>(dstFilePath, digest, AiUtilVersion.CurrentVersion, cancel: cancel);
            if (okParsed.IsOk && okParsed.Value != null)
            {
                return (okParsed.Value, dstFilePath);
            }
        }

        await Lfs.DeleteFileIfExistsAsync(dstFilePath, cancel: cancel);
        await Lfs.EnsureCreateDirectoryForFileAsync(dstFilePath, cancel: cancel);

        string voiceWavTmpPath = await Lfs.GenerateUniqueTempFilePathAsync("voicefile", ".wav", cancel: cancel);

        await FfMpeg.EncodeAudioAsync(srcVoiceFilePath, voiceWavTmpPath, FfMpegAudioCodec.Wav, useOkFile: false, cancel: cancel);

        string bgmWavTmpPath = await Lfs.GenerateUniqueTempFilePathAsync("bgmfile", ".wav", cancel: cancel);

        var retSrcList = await CreateRandomBgmFileAsync(settings.Value.SrcBgmDirOrFilePath, bgmWavTmpPath, srcDurationMsecs, settings.Value, settings.Value.TailFadeoutSecs, adjustDelta, boostWithLength, cancel: cancel);

        List<AiWaveConcatenatedSrcWavList> overwriteSrcList = new();

        // 音楽の一部をリプレース処理する
        if (replaceRanges != null && replaceRanges.Any())
        {
            HugeMemoryBuffer<byte> targetData2;
            WaveFormat targetWaveFormat;
            await using (var targetWavStream = File.Open(bgmWavTmpPath, FileMode.Open, FileAccess.Read, FileShare.Read))
            {
                await using var targetWavReader = new WaveFileReader(targetWavStream);
                targetWaveFormat = targetWavReader.WaveFormat;
                CheckWavFormat(targetWaveFormat);

                targetData2 = await targetWavStream._ReadToEndHugeAsync();
            }

            var replaceSrcWavFiles = (await Lfs.EnumDirectoryAsync(settings.Value.ReplaceBgmDirPath, true, cancel: cancel)).Where(x => x.IsFile && x.Name._IsExtensionMatch(".wav"));
            ShuffledEndlessQueue<string> q = new(replaceSrcWavFiles.Select(x => x.FullPath));

            foreach (var range in replaceRanges)
            {
                double requireLength = range.Length + range.FadeIn;

                string srcWavPath = "";
                double srcWavLength = 0;

                for (int fail = 0; fail <= 1000; fail++)
                {
                    string tmp1 = q.Dequeue();

                    srcWavLength = await GetWavFileLengthSecsAsync(tmp1, cancel: cancel);
                    if (srcWavLength >= requireLength)
                    {
                        srcWavPath = tmp1;
                        break;
                    }
                }

                if (srcWavPath._IsEmpty())
                {
                    continue;
                }

                var item = new AiWaveConcatenatedSrcWavList
                {
                    WavFilePath = srcWavPath,
                };

                // 置換 wav ファイルの読み込み
                double srcStartPosition = (srcWavLength - requireLength) * Util.RandDouble0To1();

                Memory<byte> srcMemory;

                await using (var srcWavStream = File.Open(srcWavPath, FileMode.Open, FileAccess.Read, FileShare.Read))
                {
                    await using var srcWavReader = new WaveFileReader(srcWavStream);

                    CheckWavFormat(srcWavReader.WaveFormat);

                    // 波形フォーマット前提値
                    const int sampleRate = 44100;
                    const int channels = 2;
                    const int bitsPerSample = 16;
                    // 1サンプル(1ch)あたりのバイト数
                    const int bytesPerSample = bitsPerSample / 8; // = 2
                                                                  // 1フレーム(全ch合計)あたりのバイト数(2ch前提)
                    const int blockAlign = channels * bytesPerSample; // 4バイト
                                                                      // 合成を開始するフレーム位置(整数)
                    long sourceStartFrame = (long)(srcStartPosition * sampleRate);

                    // 合成するフレーム数(整数)
                    long framesToProcess = (long)(requireLength * sampleRate);
                    if (framesToProcess <= 0)
                    {
                        continue;
                    }

                    // バイト単位での開始位置
                    long sourceStartByte = sourceStartFrame * blockAlign;
                    int bytesToProcess = (int)(framesToProcess * blockAlign);

                    srcWavReader.Seek(sourceStartByte, SeekOrigin.Begin);

                    srcMemory = await srcWavReader._ReadAllAsync(bytesToProcess, cancel: cancel);
                }

                if (settings.Value.CreateAudioEffectFilterForOverwriteMusicPartProc != null)
                {
                    var filter = settings.Value.CreateAudioEffectFilterForOverwriteMusicPartProc(cancel);
                    if (filter != null)
                    {
                        filter.PerformFilterFunc(srcMemory, cancel);
                        item.FilterName = filter.FilterName;
                        item.FilterSpeedType = filter.FilterSpeedType;
                        item.FilterSettings = filter.EffectSettings._ToJObject();
                        item.TargetStartMsec = (int)(range.StartPosition * 1000);
                        item.TargetEndMsec = (int)((range.StartPosition + range.Length) * 1000);
                        item.SourceStartMsec = (int)(srcStartPosition * 1000);
                        item.SourceEndMsec = (int)((srcStartPosition + requireLength) * 1000);
                    }
                }

                // 合成処理
                ReplaceWavDataWithFadeInOut(targetData2, srcMemory, range.StartPosition, range.FadeIn, range.FadeOut);

                overwriteSrcList.Add(item);
            }

            string bgmWavTmpPath2 = await Lfs.GenerateUniqueTempFilePathAsync("tmppath", ".wav", cancel: cancel);

            // 結果を wav に書き出す
            await using (var targetWavStream = File.Create(bgmWavTmpPath2))
            {
                await using var writer = new WaveFileWriter(targetWavStream, targetWaveFormat);

                await writer.FlushAsync(cancel);

                await Util.CopyHugeMemoryBufferToStreamAsync(targetWavStream, targetData2, cancel: cancel);
            }

            bgmWavTmpPath = bgmWavTmpPath2;
        }

        string voiceWavTmpPath2 = await Lfs.GenerateUniqueTempFilePathAsync("voiceWavTmpPath2", ".wav");

        // voiceWavTmpPath の後処理
        {
            //Memory<byte> voiceWavData;
            HugeMemoryBuffer<byte> voiceWavData2;
            WaveFormat voiceWavFormat;
            await using (var voiceWavStream = File.Open(voiceWavTmpPath, FileMode.Open, FileAccess.Read, FileShare.Read))
            {
                await using var voiceWavReader = new WaveFileReader(voiceWavStream);
                voiceWavFormat = voiceWavReader.WaveFormat;
                CheckWavFormat(voiceWavFormat);

                //voiceWavData = await voiceWavStream._ReadToEndAsync();
                voiceWavData2 = await voiceWavStream._ReadToEndHugeAsync(cancel: cancel);
            }

            if (okFileMeta.Value?.Options_VoiceSegmentsList != null)
            {
                foreach (var seg in okFileMeta.Value?.Options_VoiceSegmentsList)
                {
                    if (seg.DataLength != 0 && seg.IsBlank == false && seg.IsTag == false)
                    {
                        if (seg.Level >= 1)
                        {
                            AiAudioEffectSpeedType speedType = AiAudioEffectSpeedType.Light;

                            if (seg.Level == 2)
                            {
                                speedType = AiAudioEffectSpeedType.Normal;
                            }
                            else if (seg.Level >= 3)
                            {
                                speedType = AiAudioEffectSpeedType.Heavy;
                            }

                            var effect = AiAudioEffectCollection.AllCollectionRandomQueue.Dequeue();

                            AiAudioEffectFilter filter = new AiAudioEffectFilter(effect, speedType, 1.5 + (double)seg.Level, cancel: cancel);

                            long dataStartPositionInBytes = AiWaveUtil.GetWavDataPositionInByteFromTime(seg.TimePosition, voiceWavFormat);
                            long dataEndPositionInBytes = AiWaveUtil.GetWavDataPositionInByteFromTime(seg.TimePosition + seg.TimeLength, voiceWavFormat);
                            long dataLengthInBytes = dataEndPositionInBytes - dataStartPositionInBytes;

                            if (voiceWavData2.Length < (dataStartPositionInBytes + dataLengthInBytes))
                            {
                            }
                            else
                            {
                                Memory<byte> voiceProcessTargetMemory = new byte[dataLengthInBytes];
                                int sz = await voiceWavData2.ReadRandomAsync(dataStartPositionInBytes, voiceProcessTargetMemory, cancel);
                                if (sz != dataLengthInBytes)
                                {
                                    throw new CoresLibException("sz != dataLengthInBytes");
                                }

                                //var voiceProcessTarget = voiceWavData.Slice(dataStartPositionInBytes, dataLengthInBytes);

                                filter.PerformFilterFunc(voiceProcessTargetMemory, cancel);

                                await voiceWavData2.WriteRandomAsync(dataStartPositionInBytes, voiceProcessTargetMemory, cancel);

                                seg.FilterName = filter.FilterName;
                                seg.FilterSettings = filter.EffectSettings._ToJObject();
                                seg.FilterSpeedType = filter.FilterSpeedType;
                            }
                        }
                    }
                }
            }

            // 結果を wav に書き出す
            await using (var voiceWavTmpPath2Stream = File.Create(voiceWavTmpPath2))
            {
                await using var writer = new WaveFileWriter(voiceWavTmpPath2Stream, voiceWavFormat);

                await writer.FlushAsync(cancel);

                await Util.CopyHugeMemoryBufferToStreamAsync(voiceWavTmpPath2Stream, voiceWavData2, cancel: cancel);
            }
        }

        string outWavTmpPath = await Lfs.GenerateUniqueTempFilePathAsync("bgmadded_file", ".wav", cancel: cancel);

        await FfMpeg.AddBgmToVoiceFileAsync(voiceWavTmpPath2, bgmWavTmpPath, outWavTmpPath, smoothMode: settings.Value.SmoothMode, cancel: cancel);

        var parsed = await FfMpeg.EncodeAudioAsync(outWavTmpPath, dstFilePath, codec, kbps, useOkFile: false, metaData: newMeta, cancel: cancel);

        parsed.Options_UsedBgmSrcMusicList = retSrcList;
        parsed.Options_UsedOverwriteSrcMusicList = overwriteSrcList;
        parsed.Options_VoiceSegmentsList = okFileMeta.Value?.Options_VoiceSegmentsList;

        if (useOkFile)
        {
            await Lfs.WriteOkFileAsync(dstFilePath, parsed, digest, AiUtilVersion.CurrentVersion, cancel);
        }

        await Lfs.DeleteFileIfExistsAsync(voiceWavTmpPath, cancel: cancel);
        await Lfs.DeleteFileIfExistsAsync(voiceWavTmpPath2, cancel: cancel);
        await Lfs.DeleteFileIfExistsAsync(bgmWavTmpPath, cancel: cancel);
        await Lfs.DeleteFileIfExistsAsync(outWavTmpPath, cancel: cancel);

        return (parsed, dstFilePath);
    }



    public async Task AddRandomMaterialsToAllVoiceAndAudioFilesAsync(string srcVoiceAudioFilePath, string dstDirRoot, string srcMaterialsDirPath, FfMpegAudioCodec codec,
        AiRandomBgmSettingsFactory settingsFactory,
        int kbps = 0, string? oldTagStr = null, string? newTagStr = null, object? userParam = null, CancellationToken cancel = default)
    {
        var srcFiles = await Lfs.EnumDirectoryAsync(srcVoiceAudioFilePath, true, cancel: cancel);

        foreach (var srcFile in srcFiles.Where(x => x.IsFile && x.Name._IsExtensionMatch(Consts.Extensions.Filter_MusicFiles) && x.Name._InStri("bgm_x1.00") && PP.GetFileName(PP.GetDirectoryName(x.FullPath)).StartsWith("_") == false)
            .OrderBy(x => x.FullPath, StrCmpi)._Shuffle().ToList())
        {
            cancel.ThrowIfCancellationRequested();

            try
            {
                string relativeDirPth = PP.GetRelativeDirectoryName(PP.GetDirectoryName(srcFile.FullPath), srcVoiceAudioFilePath);

                string dstDirPath = PP.Combine(dstDirRoot, relativeDirPth);

                var settingsList = settingsFactory.GetBgmSettingListProc(srcFile.FullPath, userParam);

                HashSet<string> keyNames = new HashSet<string>(StrCmpi);
                settingsList._DoForEach(x => keyNames.Add(x.Key));

                foreach (var key in keyNames.OrderBy(x => x, StrCmpi))
                {
                    Con.WriteLine($"Add Random Materials: '{srcFile.FullPath}' -> '{dstDirPath}'");

                    var bgmSettings = settingsList.Where(x => x.Key._IsSamei(key)).First();

                    var result = await AddRandomMaterialsToVoiceAndAudioFileAsync(srcFile.FullPath, dstDirPath,
                        bgmSettings, codec, kbps, oldTagStr, newTagStr, cancel: cancel);
                }
            }
            catch (Exception ex)
            {
                ex._Error();
            }
        }
    }

    public async Task<(FfMpegParsedList Parsed, string DestFileName)> AddRandomMaterialsToVoiceAndAudioFileAsync(
        string srcVoiceAudioFilePath, string dstDir, KeyValuePair<string, AiRandomBgmSettings> settings, FfMpegAudioCodec codec,
        int kbps = 0, string? oldTagStr = null, string? newTagStr = null, CancellationToken cancel = default)
    {
        var srcVoiceAudioFileMetaData = await FfMpeg.ReadMetaDataWithFfProbeAsync(srcVoiceAudioFilePath, cancel: cancel);

        srcVoiceAudioFileMetaData.ReParseMain();

        int srcDurationMsecs = srcVoiceAudioFileMetaData.Input?.GetDurationMsecs() ?? -1;

        if (srcDurationMsecs < 0)
        {
            throw new CoresLibException($"Failed to get duration '{srcVoiceAudioFilePath}'");
        }

        MediaMetaData srcMeta = srcVoiceAudioFileMetaData.Meta;
        if (srcMeta == null) srcMeta = new MediaMetaData();

        var okFileMeta = await Lfs.ReadOkFileAsync<FfMpegParsedList>(srcVoiceAudioFilePath, cancel: cancel);

        if (okFileMeta.IsError || okFileMeta.Value == null || okFileMeta.Value.Options_VoiceSegmentsList == null)
        {
            // Unsupported. Skip;
            return default;
        }

        if (okFileMeta.IsOk && okFileMeta.Value != null && okFileMeta.Value.Meta != null)
        {
            okFileMeta.Value.ReParseMain();

            if (okFileMeta.Value.Meta.HasValue())
            {
                srcMeta = okFileMeta.Value.Meta;
            }
        }

        MediaMetaData newMeta = srcMeta._CloneDeep();

        string newTagStrAddForKey = "";
        if (settings.Key._IsFilled())
        {
            newTagStrAddForKey += "_" + settings.Key;
        }

        if (oldTagStr._IsFilled() && newTagStr._IsFilled())
        {
            newTagStr += newTagStrAddForKey;

            newMeta.Album = newMeta.Album._ReplaceStr(oldTagStr, newTagStr, true);
            newMeta.AlbumArtist = newMeta.AlbumArtist._ReplaceStr(oldTagStr, newTagStr, true);
            newMeta.Title = newMeta.Title._ReplaceStr(oldTagStr, newTagStr, true);
            newMeta.Artist = newMeta.Artist._ReplaceStr(oldTagStr, newTagStr, true);
        }
        else
        {
            string bgm_x_replace_src_str = $" - bgm{newTagStrAddForKey}_x";
            string bgm_x_replace_dst_str = $" - eff{newTagStrAddForKey}_x";

            newMeta.Album = newMeta.Album._ReplaceStr(bgm_x_replace_src_str, bgm_x_replace_dst_str, true);
            newMeta.AlbumArtist = newMeta.AlbumArtist._ReplaceStr(bgm_x_replace_src_str, bgm_x_replace_dst_str, true);
            newMeta.Title = newMeta.Title._ReplaceStr(bgm_x_replace_src_str, bgm_x_replace_dst_str, true);
            newMeta.Artist = newMeta.Artist._ReplaceStr(bgm_x_replace_src_str, bgm_x_replace_dst_str, true);
        }

        string dstFileName;
        string dstExtension = FfMpegUtil.GetExtensionFromCodec(codec);

        if (oldTagStr._IsFilled() && newTagStr._IsFilled())
        {
            dstFileName = PP.GetFileNameWithoutExtension(srcVoiceAudioFilePath)._ReplaceStr(oldTagStr, newTagStr) + dstExtension;
        }
        else
        {
            string tmp1 = PP.GetFileNameWithoutExtension(srcVoiceAudioFilePath);
            if (tmp1.StartsWith($"bgm{newTagStrAddForKey}_", StrCmpi)) tmp1 = tmp1.Substring(4); // ファイル名から "bgm_" を消す
            string tmp2 = $"eff{newTagStrAddForKey}_" + tmp1;

            dstFileName = tmp2 + dstExtension;
        }

        string dstFilePath = PP.Combine(dstDir, dstFileName);

        if (dstFilePath._IsSamei(srcVoiceAudioFilePath))
        {
            throw new CoresLibException($"dstFilePath == srcVoiceFilePath: '{srcVoiceAudioFilePath}'");
        }

        var parsed = await this.CompositAudioFileByAcxBcxTagsWithManyWavMaterialsAsync(srcVoiceAudioFilePath, dstFilePath, codec, kbps, settings.Value.CompositWaveSettings, 1.0, newMeta, cancel: cancel);

        return (parsed, dstFilePath);
    }



    public async Task MakeMultipleMixMusicsFromMultipleMusics(IEnumerable<string> srcWavFilesPathList, string destDirPath, string albumName, FfMpegAudioCodec codec = FfMpegAudioCodec.Aac, int kbps = 0, int numRotate = 1, int numPartsMeyasu = 6, int targetLengthMsecs = 6 * 60 * 1000, int mixFadeMsecs = 12 * 1000, CancellationToken cancel = default)
    {
        List<string> ret = new List<string>();

        if (numRotate <= 0) numRotate = 1;

        ShuffledEndlessQueue<string> additionalWavPathQueue = new(srcWavFilesPathList, avoidLastSame: true);

        Queue<(int RotateIndex, string WavPath1, string ArtistName, string MusicTitle)> queue = new();

        for (int i = 0; i < numRotate; i++)
        {
            var tmp = srcWavFilesPathList.ToArray()._Shuffle().ToArray();

            foreach (var srcFullPath in tmp)
            {
                string[] tokens = PP.GetFileNameWithoutExtension(srcFullPath)._Split(StringSplitOptions.TrimEntries, " - ");

                if (tokens.Length == 3)
                {
                    queue.Enqueue((i + 1, srcFullPath, tokens[1], tokens[2]));
                }
            }
        }

        await Lfs.CreateDirectoryAsync(destDirPath, cancel: cancel);

        int trackNumber = 0;

        while (queue.TryDequeue(out var music))
        {
            trackNumber++;

            try
            {
                string dstFilePath = PP.Combine(destDirPath, $"[{albumName}_{(trackNumber).ToString("D4")}] - {music.ArtistName} - {music.MusicTitle} - r{music.RotateIndex}{FfMpegUtil.GetExtensionFromCodec(codec)}");

                Con.WriteLine(dstFilePath);

                List<ReadOnlyMemory<byte>> srcDataList = new();

                var srcData1 = (await Lfs.ReadDataFromFileAsync(music.WavPath1, 100 * 1024 * 1024, cancel: cancel))._AsReadOnlyMemory();
                srcDataList.Add(srcData1);

                for (int i = 0; i < numPartsMeyasu; i++)
                {
                    var additionalSrcData = (await Lfs.ReadDataFromFileAsync(additionalWavPathQueue.Dequeue(), 100 * 1024 * 1024, cancel: cancel))._AsReadOnlyMemory();

                    srcDataList.Add(additionalSrcData);
                }

                //var dstData = AiGenerateExactLengthMusicLib.GenerateExactLengthMusic(srcData1, targetLengthMsecs, mixFadeMsecs);
                var dstData = AiMixTwoMusicsLib2.MixTwoMusicsMultiple(srcDataList, numPartsMeyasu, targetLengthMsecs, 30 * 1000);

                string dstTmpFileName = await Lfs.GenerateUniqueTempFilePathAsync("aarmp1", ".wav", cancel: cancel);

                await Lfs.WriteDataToFileAsync(dstTmpFileName, dstData, cancel: cancel);

                MediaMetaData meta = new MediaMetaData
                {
                    Track = trackNumber,
                    TrackTotal = 9999,
                    Album = $"{albumName}",
                    Artist = $"[{albumName}] - {music.ArtistName}",
                    Title = $"[{albumName}_{trackNumber.ToString("D4")}] - {music.MusicTitle}",
                };

                string encodeDstTmpFilePath = await Lfs.GenerateUniqueTempFilePathAsync("ttamp1", FfMpegUtil.GetExtensionFromCodec(codec), cancel: cancel);

                var res = await FfMpeg.EncodeAudioAsync(dstTmpFileName, encodeDstTmpFilePath, codec, kbps, 100, meta, $"t{(trackNumber).ToString("D4")} - r{music.RotateIndex} - {music.ArtistName} - {music.MusicTitle}", useOkFile: false, cancel: cancel);

                await Lfs.CopyFileAsync(encodeDstTmpFilePath, dstFilePath, cancel: cancel);

                ret.Add(dstFilePath);

                await Lfs.DeleteFileIfExistsAsync(dstTmpFileName, cancel: cancel);
                await Lfs.DeleteFileIfExistsAsync(encodeDstTmpFilePath, cancel: cancel);
            }
            catch (Exception ex)
            {
                ex._Error();
            }
        }
    }


    public async Task MakeMultipleMixMusics(IEnumerable<string> srcWavFilesPathList, string destDirPath, string albumName, FfMpegAudioCodec codec = FfMpegAudioCodec.Aac, int kbps = 0, int numRotate = 1, int numPartsMeyasu = 6, int targetLengthMsecs = 6 * 60 * 1000, int mixFadeMsecs = 12 * 1000, CancellationToken cancel = default)
    {
        List<string> ret = new List<string>();

        if (numRotate <= 0) numRotate = 1;

        ShuffledEndlessQueue<string> additionalWavPathQueue = new(srcWavFilesPathList, avoidLastSame: true);

        Queue<(int RotateIndex, string WavPath1, string ArtistName, string MusicTitle, string WavPath2, string WavPath3)> queue = new();

        for (int i = 0; i < numRotate; i++)
        {
            var tmp = srcWavFilesPathList.ToArray()._Shuffle().ToArray();

            foreach (var srcFullPath in tmp)
            {
                string[] tokens = PP.GetFileNameWithoutExtension(srcFullPath)._Split(StringSplitOptions.TrimEntries, " - ");

                if (tokens.Length == 3)
                {
                    queue.Enqueue((i + 1, srcFullPath, tokens[1], tokens[2], additionalWavPathQueue.Dequeue(), additionalWavPathQueue.Dequeue()));
                }
            }
        }

        await Lfs.CreateDirectoryAsync(destDirPath, cancel: cancel);

        int trackNumber = 0;

        while (queue.TryDequeue(out var music))
        {
            trackNumber++;

            try
            {
                string dstFilePath = PP.Combine(destDirPath, $"[{albumName}_{(trackNumber).ToString("D4")}] - {music.ArtistName} - {music.MusicTitle} - r{music.RotateIndex}{FfMpegUtil.GetExtensionFromCodec(codec)}");

                Con.WriteLine(dstFilePath);

                var srcData1 = (await Lfs.ReadDataFromFileAsync(music.WavPath1, 100 * 1024 * 1024, cancel: cancel))._AsReadOnlyMemory();
                var srcData2 = (await Lfs.ReadDataFromFileAsync(music.WavPath2, 100 * 1024 * 1024, cancel: cancel))._AsReadOnlyMemory();
                var srcData3 = (await Lfs.ReadDataFromFileAsync(music.WavPath3, 100 * 1024 * 1024, cancel: cancel))._AsReadOnlyMemory();

                //var dstData = AiGenerateExactLengthMusicLib.GenerateExactLengthMusic(srcData1, targetLengthMsecs, mixFadeMsecs);
                var dstData = AiMixTwoMusicsLib2.MixTwoMusicsMultiple(new[] { srcData1, srcData2, srcData3 }, numPartsMeyasu, targetLengthMsecs, 30 * 1000);

                string dstTmpFileName = await Lfs.GenerateUniqueTempFilePathAsync("aarmp1", ".wav", cancel: cancel);

                await Lfs.WriteDataToFileAsync(dstTmpFileName, dstData, cancel: cancel);

                MediaMetaData meta = new MediaMetaData
                {
                    Track = trackNumber,
                    TrackTotal = 9999,
                    Album = $"{albumName}",
                    Artist = $"[{albumName}] - {music.ArtistName}",
                    Title = $"[{albumName}_{trackNumber.ToString("D4")}] - {music.MusicTitle}",
                };

                string encodeDstTmpFilePath = await Lfs.GenerateUniqueTempFilePathAsync("ttamp1", FfMpegUtil.GetExtensionFromCodec(codec), cancel: cancel);

                var res = await FfMpeg.EncodeAudioAsync(dstTmpFileName, encodeDstTmpFilePath, codec, kbps, 100, meta, $"t{(trackNumber).ToString("D4")} - r{music.RotateIndex} - {music.ArtistName} - {music.MusicTitle}", useOkFile: false, cancel: cancel);

                await Lfs.CopyFileAsync(encodeDstTmpFilePath, dstFilePath, cancel: cancel);

                ret.Add(dstFilePath);

                await Lfs.DeleteFileIfExistsAsync(dstTmpFileName, cancel: cancel);
                await Lfs.DeleteFileIfExistsAsync(encodeDstTmpFilePath, cancel: cancel);
            }
            catch (Exception ex)
            {
                ex._Error();
            }
        }
    }


    public async Task ConvertAllMusicToExactMinsAsync(IEnumerable<string> srcWavFilesPathList, string destDirPath, string albumName, FfMpegAudioCodec codec = FfMpegAudioCodec.Aac, int kbps = 0, int numRotate = 1, int targetLengthMsecs = 6 * 60 * 1000, int mixFadeMsecs = 10 * 1000, CancellationToken cancel = default)
    {
        List<string> ret = new List<string>();

        if (numRotate <= 0) numRotate = 1;

        Queue<(int RotateIndex, string WavPath, string ArtistName, string MusicTitle)> queue = new();

        for (int i = 0; i < numRotate; i++)
        {
            var tmp = srcWavFilesPathList.ToArray()._Shuffle().ToArray();

            foreach (var srcFullPath in tmp)
            {
                string[] tokens = PP.GetFileNameWithoutExtension(srcFullPath)._Split(StringSplitOptions.TrimEntries, " - ");

                if (tokens.Length == 3)
                {
                    queue.Enqueue((i + 1, srcFullPath, tokens[1], tokens[2]));
                }
            }
        }

        await Lfs.CreateDirectoryAsync(destDirPath, cancel: cancel);

        int trackNumber = 0;

        while (queue.TryDequeue(out var music))
        {
            trackNumber++;

            try
            {
                string dstFilePath = PP.Combine(destDirPath, $"[{albumName}_{(trackNumber).ToString("D4")}] - {music.ArtistName} - {music.MusicTitle} - r{music.RotateIndex}{FfMpegUtil.GetExtensionFromCodec(codec)}");

                Con.WriteLine(dstFilePath);

                var srcData = await Lfs.ReadDataFromFileAsync(music.WavPath, 100 * 1024 * 1024, cancel: cancel);

                var dstData = AiGenerateExactLengthMusicLib.GenerateExactLengthMusic(srcData, targetLengthMsecs, mixFadeMsecs);

                string dstTmpFileName = await Lfs.GenerateUniqueTempFilePathAsync("aarmp1", ".wav", cancel: cancel);

                await Lfs.WriteDataToFileAsync(dstTmpFileName, dstData, cancel: cancel);

                MediaMetaData meta = new MediaMetaData
                {
                    Track = trackNumber,
                    TrackTotal = 9999,
                    Album = $"{albumName}",
                    Artist = $"[{albumName}] - {music.ArtistName}",
                    Title = $"[{albumName}_{trackNumber.ToString("D4")}] - {music.MusicTitle}",
                };

                string encodeDstTmpFilePath = await Lfs.GenerateUniqueTempFilePathAsync("ttamp1", FfMpegUtil.GetExtensionFromCodec(codec), cancel: cancel);

                var res = await FfMpeg.EncodeAudioAsync(dstTmpFileName, encodeDstTmpFilePath, codec, kbps, 100, meta, $"t{(trackNumber).ToString("D4")} - r{music.RotateIndex} - {music.ArtistName} - {music.MusicTitle}", useOkFile: false, cancel: cancel);

                await Lfs.CopyFileAsync(encodeDstTmpFilePath, dstFilePath, cancel: cancel);

                ret.Add(dstFilePath);

                await Lfs.DeleteFileIfExistsAsync(dstTmpFileName, cancel: cancel);
                await Lfs.DeleteFileIfExistsAsync(encodeDstTmpFilePath, cancel: cancel);
            }
            catch (Exception ex)
            {
                ex._Error();
            }
        }
    }

    public async Task<List<string>> CreateManyMusicMixAsync(DateTimeOffset timeStamp,
        IEnumerable<string> srcWavFilesPathList, string destDirPath, string albumName, string artist, AiRandomBgmSettings settings,
        FfMpegAudioCodec codec = FfMpegAudioCodec.Aac, int kbps = 0, int numRotate = 1, int minTracks = 1,
        int durationOfSingleFileMsecs = 3 * 60 * 60 * 1000, bool simpleFileName = false,
        bool boostWithLength = false,
        CancellationToken cancel = default)
    {
        List<string> ret = new List<string>();

        if (numRotate <= 0) numRotate = 1;
        if (minTracks <= 0) minTracks = 1;

        if (boostWithLength)
        {
            List<string> tmp1 = new();

            // 曲の長さ 1 分あたり 1 個をリストに追加
            foreach (var path in srcWavFilesPathList)
            {
                try
                {
                    long size = (await Lfs.GetFileMetadataAsync(path, cancel: cancel)).Size;
                    double minutes = (double)size / 10584000.0;
                    int minutes_int = Math.Max((int)minutes, 1);

                    for (int i = 0; i < minutes_int; i++)
                    {
                        tmp1.Add(path);
                    }
                }
                catch (Exception ex)
                {
                    ex._Error();
                }
            }

            Con.WriteLine($"Boost. Original: {srcWavFilesPathList.Count()._ToString3()} files, Boosted: {tmp1.Count._ToString3()} files.");

            srcWavFilesPathList = tmp1.ToArray();
        }

        ShuffledEndlessQueue<string> q = new ShuffledEndlessQueue<string>(srcWavFilesPathList);

        string timestampStr = "[" + timeStamp._ToYymmddStr(yearTwoDigits: true) + "_" + timeStamp._ToHhmmssStr().Substring(0, 4) + "]";

        string secstr = $"{(settings.Medley_SingleFilePartMSecs / 1000)}s";

        await Lfs.CreateDirectoryAsync(destDirPath, cancel: cancel);

        for (int i = 0; i < 999; i++)
        {
            if (q.NumRotate >= numRotate && i >= minTracks)
            {
                break;
            }

            int rotateForDisplay = q.NumRotate + 1;
            if (rotateForDisplay <= 0) rotateForDisplay = 1;

            string dstFilePath;

            if (simpleFileName == false)
            {
                dstFilePath = PP.Combine(destDirPath, $"{albumName} - {artist} - {timestampStr} - Track_{(i + 1).ToString("D3")} - r{rotateForDisplay} - {secstr}{FfMpegUtil.GetExtensionFromCodec(codec)}");
            }
            else
            {
                dstFilePath = PP.Combine(destDirPath, $"{albumName} - Track_{(i + 1).ToString("D3")} - r{rotateForDisplay} - {secstr}{FfMpegUtil.GetExtensionFromCodec(codec)}");
            }

            if (await Lfs.IsFileExistsAsync(dstFilePath, cancel))
            {
                Con.WriteLine($"File '{dstFilePath}' already exists. Skip this artist at all.");
                return new();
            }

            Con.WriteLine($"--- Concat {albumName} - {artist} Track #{(i + 1).ToString("D3")} (NumRotate = {q.NumRotate})");

            string dstTmpFileName = await Lfs.GenerateUniqueTempFilePathAsync("concat3", ".wav", cancel: cancel);

            var srcFilesList = await ConcatWavFileFromRandomDirAsync(q, dstTmpFileName, durationOfSingleFileMsecs, settings, cancel);

            MediaMetaData meta = new MediaMetaData
            {
                Track = i + 1,
                TrackTotal = 999,
                Album = $"{albumName} - {timestampStr}",
                Artist = $"{albumName} - {artist} - {timestampStr} - {secstr}",
                Title = $"{albumName} - {artist} - Track_{(i + 1).ToString("D3")} - {secstr} - r{rotateForDisplay}",
            };

            if (simpleFileName)
            {
                meta.Title = $"{albumName} - Track_{(i + 1).ToString("D3")} - {secstr} - r{rotateForDisplay}";
            }

            Con.WriteLine($"--- Concat {albumName} - {artist} Track #{(i + 1).ToString("D3")} (NumRotate = {q.NumRotate})");

            string encodeDstTmpFilePath = await Lfs.GenerateUniqueTempFilePathAsync("encode3", FfMpegUtil.GetExtensionFromCodec(codec), cancel: cancel);

            var res = await FfMpeg.EncodeAudioAsync(dstTmpFileName, encodeDstTmpFilePath, codec, kbps, 100, meta, $"{artist} - Track_{(i + 1).ToString("D3")}", sourceFilePathList: srcFilesList, useOkFile: false, cancel: cancel);

            await Lfs.CopyFileAsync(encodeDstTmpFilePath, dstFilePath, cancel: cancel);

            await Lfs.WriteOkFileAsync(dstFilePath, res, version: AiUtilVersion.CurrentVersion, cancel: cancel);

            ret.Add(dstFilePath);

            await Lfs.DeleteFileIfExistsAsync(dstTmpFileName, cancel: cancel);
            await Lfs.DeleteFileIfExistsAsync(encodeDstTmpFilePath, cancel: cancel);
        }

        return ret;
    }


    public async Task<List<AiWaveConcatenatedSrcWavList>> CreateRandomBgmFileAsync(string srcMusicWavsDirOrFilePath, string dstWavFilePath, int totalDurationMsecs, AiRandomBgmSettings settings, int fadeOutSecs, double adjustDelta, bool boostWithLength, CancellationToken cancel = default)
    {
        string dstTmpFileName = await Lfs.GenerateUniqueTempFilePathAsync("concat2", ".wav", cancel: cancel);

        List<AiWaveConcatenatedSrcWavList> retSrcList = await ConcatWavFileFromRandomDirAsync(srcMusicWavsDirOrFilePath, dstTmpFileName, totalDurationMsecs, settings, fadeOutSecs, boostWithLength, cancel: cancel);

        await FfMpeg.AdjustAudioVolumeAsync(dstTmpFileName, dstWavFilePath, adjustDelta, cancel);

        await Lfs.DeleteFileIfExistsAsync(dstTmpFileName, cancel: cancel);

        return retSrcList;
    }

    public async Task<List<AiWaveConcatenatedSrcWavList>> ConcatWavFileFromRandomDirAsync(string srcMusicWavsDirOrFilePath, string dstWavFilePath, int totalDurationMsecs, AiRandomBgmSettings settings, int fadeOutSecs, bool boostWithLength, CancellationToken cancel = default)
    {
        FileSystemEntity[] srcWavList = await Lfs.EnumDirectoryAsync(srcMusicWavsDirOrFilePath, true, flags: EnumDirectoryFlags.AllowDirectFilePath, cancel: cancel);

        List<string> fileNamesList = new List<string>();

        foreach (var srcWav in srcWavList.Where(x => x.IsFile && x.Name._IsExtensionMatch(".wav")).OrderBy(x => x.FullPath, StrCmpi))
        {
            if (await Lfs.IsOkFileExistsAsync(srcWav.FullPath, ifOkDirNotExistsRetOk: true, cancel: cancel))
            {
                fileNamesList.Add(srcWav.FullPath);
            }
        }

        if (boostWithLength)
        {
            List<string> tmp1 = new();

            // 曲の長さ 1 分あたり 1 個をリストに追加
            foreach (var path in fileNamesList)
            {
                try
                {
                    long size = (await Lfs.GetFileMetadataAsync(path, cancel: cancel)).Size;
                    double minutes = (double)size / 10584000.0;
                    int minutes_int = Math.Max((int)minutes, 1);

                    for (int i = 0; i < minutes_int; i++)
                    {
                        tmp1.Add(path);
                    }
                }
                catch (Exception ex)
                {
                    ex._Error();
                }
            }

            Con.WriteLine($"Boost. Original: {fileNamesList.Count()._ToString3()} files, Boosted: {tmp1.Count._ToString3()} files.");

            fileNamesList = tmp1.ToList();
        }


        ShuffledEndlessQueue<string> srcQueue = new ShuffledEndlessQueue<string>(fileNamesList);

        return await ConcatWavFileFromRandomDirAsync(srcQueue, dstWavFilePath, totalDurationMsecs, settings, cancel);
    }

    public async Task<List<AiWaveConcatenatedSrcWavList>> ConcatWavFileFromRandomDirAsync(ShuffledEndlessQueue<string> srcMusicWavsFilePathQueue, string dstWavFilePath, int totalDurationMsecs, AiRandomBgmSettings settings, CancellationToken cancel = default)
    {
        List<AiWaveConcatenatedSrcWavList> retSrcList;

        string dstTmpFileName;

        if (settings.TailFadeoutSecs <= 0)
        {
            dstTmpFileName = dstWavFilePath;
        }
        else
        {
            dstTmpFileName = await Lfs.GenerateUniqueTempFilePathAsync("concat", ".wav", cancel: cancel);
        }

        if (settings.Medley == false)
        {
            retSrcList = await ConcatWavFileFromQueueAsync(srcMusicWavsFilePathQueue, totalDurationMsecs, dstTmpFileName, settings.Concat_UseTailIfSingle, cancel: cancel);
        }
        else
        {
            retSrcList = await AiWaveConcatenateWithCrossFadeUtil.ConcatenateAsync(srcMusicWavsFilePathQueue, totalDurationMsecs, settings, dstTmpFileName, cancel);
        }

        if (settings.TailFadeoutSecs > 0)
        {
            await Lfs.DeleteFileIfExistsAsync(dstWavFilePath, cancel: cancel);
            await Lfs.EnsureCreateDirectoryForFileAsync(dstWavFilePath, cancel: cancel);

            int totalSecs = totalDurationMsecs / 1000;
            int startSecs = Math.Max(totalSecs - settings.TailFadeoutSecs, 0);
            int fadeSecs = Math.Min(settings.TailFadeoutSecs, totalSecs - startSecs);

            await FfMpeg.RunFfMpegAsync($"-y -i {dstTmpFileName._EnsureQuotation()} -vn -reset_timestamps 1 -ar 44100 -ac 2 -c:a pcm_s16le " +
                $"-map_metadata -1 -f wav -af \"afade=t=out:st={startSecs}:d={fadeSecs}\" {dstWavFilePath._EnsureQuotation()}",
                cancel: cancel);

            await Lfs.DeleteFileIfExistsAsync(dstTmpFileName, cancel: cancel);
        }

        return retSrcList;
    }

    public static async Task<List<AiWaveConcatenatedSrcWavList>> ConcatWavFileFromQueueAsync(
        ShuffledEndlessQueue<string> srcWavFilesQueue,
        int totalDurationMsecs,
        string dstWavFilePath, bool useTailIfSingleFile, CancellationToken cancel = default)
    {
        List<AiWaveConcatenatedSrcWavList> retSrcList = new List<AiWaveConcatenatedSrcWavList>();

        await Lfs.DeleteFileIfExistsAsync(dstWavFilePath, cancel: cancel);
        await Lfs.EnsureCreateDirectoryForFileAsync(dstWavFilePath, cancel: cancel);

        // 1. キューが空なら何もできないので無音の WAV を生成するか、そのまま return
        if (srcWavFilesQueue.Count == 0)
        {
            // 無音で書き出したい場合は下記のように処理する
            // （フォーマットがわからないので、ここでは 44.1kHz / 16bit / stereo の例として無音を作る）
            await CreateSilentWavAsync(44100, 16, 2, totalDurationMsecs, dstWavFilePath, cancel);
            // return;

            // 何もしない場合
            return retSrcList;
        }

        // 2. まずキューの先頭を覗いてフォーマットを取得する
        string firstWavPath = srcWavFilesQueue.Peek();

        // File.Open は async メソッドがあるのでそちらを使う（NAudio ではコンストラクタが同期的だが、可能な部分のみ async 化）
        await using var firstFileStream = File.Open(firstWavPath, FileMode.Open, FileAccess.Read, FileShare.Read);
        await using var firstReader = new WaveFileReader(firstFileStream);
        WaveFormat waveFormat = firstReader.WaveFormat;
        AiTask.CheckWavFormat(waveFormat);

        // 3. 書き出し先 WaveFileWriter を準備
        //    WaveFileWriter はコンストラクタが同期的だが、FileStream は async で開く。
        await using var destFileStream = File.Create(dstWavFilePath);
        using var writer = new WaveFileWriter(destFileStream, waveFormat);

        // 4. 書き出すべき合計バイト数を計算する
        //    (サンプリング数 = サンプルレート * (totalDurationMsecs / 1000.0) を丸める)
        long totalSamples = (long)Math.Round(waveFormat.SampleRate * (totalDurationMsecs / 1000.0));
        long totalBytesToWrite = totalSamples * waveFormat.BlockAlign;
        long writtenBytes = 0;

        bool isSingleSource = (srcWavFilesQueue.RealCount == 1);

        // 5. ソース WAV ファイルを順番に読み込み、合計サイズに達するまで書き込む
        var buffer = new byte[65536];
        while (srcWavFilesQueue.Count > 0 && writtenBytes < totalBytesToWrite)
        {
            string currentWavPath = srcWavFilesQueue.Dequeue();

            await using var sourceStream = File.Open(currentWavPath, FileMode.Open, FileAccess.Read, FileShare.Read);
            await using var readerWav = new WaveFileReader(sourceStream);

            AiTask.CheckWavFormat(readerWav.WaveFormat);

            if (isSingleSource)
            {
                long thisFileTotalSamples = readerWav.Length / waveFormat.BlockAlign;
                if (thisFileTotalSamples > ((totalBytesToWrite - writtenBytes) / waveFormat.BlockAlign))
                {
                    if (useTailIfSingleFile)
                    {
                        // 元ファイルが 1 個のみの場合で、元ファイルのサイズのほうが先ファイルよりも長い場合
                        // 元ファイルの末尾部分を書き出すことにする
                        readerWav.Seek((thisFileTotalSamples - (totalBytesToWrite - writtenBytes) / waveFormat.BlockAlign) * waveFormat.BlockAlign, SeekOrigin.Begin);
                    }
                }
            }

            retSrcList.Add(new AiWaveConcatenatedSrcWavList
            {
                WavFilePath = currentWavPath,
                SourceStartMsec = (int)(AiWaveUtil.CalcWavDurationFromSizeInByte(readerWav.Position, readerWav.WaveFormat) * 1000),
                SourceEndMsec = (int)(AiWaveUtil.CalcWavDurationFromSizeInByte(readerWav.Length, readerWav.WaveFormat) * 1000),
                TargetStartMsec = (int)(AiWaveUtil.CalcWavDurationFromSizeInByte(writtenBytes, readerWav.WaveFormat) * 1000),
                TargetEndMsec = (int)(AiWaveUtil.CalcWavDurationFromSizeInByte(writtenBytes + readerWav.Length - readerWav.Position, readerWav.WaveFormat) * 1000),
            });

            int bytesRead;
            // WaveFileReader は通常同期的な Read しか提供しないが、Stream としての ReadAsync は呼び出せる場合がある。
            // ただし内部実装次第で実質同期になる可能性あり。
            while ((bytesRead = await readerWav.ReadAsync(buffer, 0, buffer.Length, cancel)) > 0
                   && writtenBytes < totalBytesToWrite)
            {
                // まだ書けるバイト数
                long bytesRemaining = totalBytesToWrite - writtenBytes;

                // 今回読み込んだ分が書き込める残り容量を超える場合は切り詰め
                if (bytesRead > bytesRemaining)
                {
                    bytesRead = (int)bytesRemaining;
                }

                // dst へ書き込み (これは async メソッド)
                await writer.WriteAsync(buffer, 0, bytesRead, cancel);

                writtenBytes += bytesRead;
            }
        }

        // 6. まだ必要なバイト数が残っていたら無音(0)で埋める
        if (writtenBytes < totalBytesToWrite)
        {
            long bytesToFill = totalBytesToWrite - writtenBytes;
            var silenceBuffer = new byte[8192];

            while (bytesToFill > 0)
            {
                int writeSize = (int)Math.Min(silenceBuffer.Length, bytesToFill);

                await writer.WriteAsync(silenceBuffer, 0, writeSize, cancel);

                bytesToFill -= writeSize;
            }
        }

        // WaveFileWriter / Stream を using により閉じる (Disposeされる) ことで
        //  WAV ヘッダが正しく書き込まれる。
        return retSrcList;
    }

    private static async Task CreateSilentWavAsync(int sampleRate, int bitsPerSample, int channels,
        int durationMsecs, string filePath, CancellationToken cancel = default)
    {
        var waveFormat = new WaveFormat(sampleRate, bitsPerSample, channels);
        long totalSamples = (long)Math.Round(waveFormat.SampleRate * (durationMsecs / 1000.0));
        long totalBytesToWrite = totalSamples * waveFormat.BlockAlign;

        await using var fs = File.Create(filePath);
        await using var writer = new WaveFileWriter(fs, waveFormat);

        var silenceBuffer = new byte[65536];
        long written = 0;
        while (written < totalBytesToWrite)
        {
            int toWrite = (int)Math.Min(silenceBuffer.Length, totalBytesToWrite - written);
            await writer.WriteAsync(silenceBuffer, 0, toWrite, cancel);
            written += toWrite;
        }
    }

}

public class AiUtilBasicSettings
{
    public string AiTest_UvrCli_BaseDir = "";
    public string AiTest_VoiceBox_ExePath = "";
    public string AiTest_VoiceBox_ExeArgs = "";
    public string AiTest_FishAudio_BaseDir = "";
    public string AiTest_SeedVc_BaseDir = "";
    public string AiTest_RealEsrgan_BaseDir = "";
    public string AiTest_TesseractOCR_Data_Dir = "";
    public double AdjustAudioTargetMaxVolume = CoresConfig.DefaultAiUtilSettings.AdjustAudioTargetMaxVolume;
    public double AdjustAudioTargetMeanVolume = CoresConfig.DefaultAiUtilSettings.AdjustAudioTargetMeanVolume;
    public int VoiceBoxLocalhostPort = Consts.Ports.VoiceVox;
    public int FishAudioLocalhostPort = Consts.Ports.FishAudio;
}

public class AiUtilRealEsrganPerformOption
{
    public string Model = "RealESRGAN_x4plus";
    public int Tile = 512;
    public int Pad = 16;
    public double OutScale = 1.0;
    public bool Skip = false;
    public bool FaceMode = false;
    public bool Fp32 = false;
    public int BatchChunkCount = 16;
}

public class AiUtilRealEsrganEngine : AiUtilBasicEngine
{
    public AiUtilRealEsrganEngine(AiUtilBasicSettings settings) : base(settings, "Real-ESRGAN", settings.AiTest_RealEsrgan_BaseDir)
    {
        try
        {
        }
        catch (Exception ex)
        {
            this._DisposeSafe(ex);
            throw;
        }
    }

    public async Task PerformAsync(string srcImgDirPath, string imgExtension, string dstImgDirPath, AiUtilRealEsrganPerformOption? option = null, CancellationToken cancel = default)
    {
        imgExtension._CheckStrFilled(nameof(imgExtension));

        if (imgExtension.StartsWith(".") == false) imgExtension = "." + imgExtension;

        option ??= new();

        await Lfs.CreateDirectoryAsync(dstImgDirPath, cancel: cancel);

        var existingFiles = (await Lfs.EnumDirectoryAsync(dstImgDirPath, false, cancel: cancel)).Where(x => x.IsFile);
        foreach (var exf in existingFiles)
        {
            await Lfs.DeleteFileIfExistsAsync(exf.FullPath, raiseException: true, cancel: cancel);
        }

        if (option.Skip)
        {
            Con.WriteLine($"Real-ESRGAN: Skip: Copying from '{srcImgDirPath}' to '{dstImgDirPath}' ...");
            var srcImgFiles2 = (await Lfs.EnumDirectoryAsync(srcImgDirPath, false, cancel: cancel)).Where(x => x.IsFile && x.Name._IsExtensionMatch(imgExtension)).OrderBy(x => x.Name, StrCmpi);
            int n2 = 0;
            foreach (var src in srcImgFiles2)
            {
                n2++;

                string fn = $"{n2:D5}" + imgExtension;
                string dstFilePath = PP.Combine(dstImgDirPath, fn);

                await Lfs.CopyFileAsync(src.FullPath, dstFilePath);
            }
            Con.WriteLine("Real-ESRGAN: Skip: Done.");
            return;
        }

        var srcImgFiles = (await Lfs.EnumDirectoryAsync(srcImgDirPath, false, cancel: cancel)).Where(x => x.IsFile && x.Name._IsExtensionMatch(imgExtension)).OrderBy(x => x.Name, StrCmpi);

        // チャンクごとに分割 (同時に大量に実行すると子プロセスがメモリリークしてエラーになるため)
        int chunkCount = option.BatchChunkCount;
        if (chunkCount <= 0) chunkCount = int.MaxValue;
        var srcImgFilesByChunk = srcImgFiles.Chunk(chunkCount).Select(x => x.ToList()).ToList();

        string pythonBatchInDir = PP.Combine(this.BaseDirPath, "dn_batch_in");
        string pythonBatchOutDir = PP.Combine(this.BaseDirPath, "dn_batch_out");

        await Lfs.CreateDirectoryAsync(pythonBatchInDir, cancel: cancel);
        await Lfs.CreateDirectoryAsync(pythonBatchOutDir, cancel: cancel);

        int n = 0;

        for (int i = 0; i < srcImgFilesByChunk.Count; i++)
        {
            var srcImgFilesOfThisChunk = srcImgFilesByChunk[i];

            // 古いファイルを全部消す
            existingFiles = (await Lfs.EnumDirectoryAsync(pythonBatchInDir, false, cancel: cancel)).Where(x => x.IsFile);
            foreach (var exf in existingFiles)
            {
                await Lfs.DeleteFileIfExistsAsync(exf.FullPath, raiseException: true, cancel: cancel);
            }

            existingFiles = (await Lfs.EnumDirectoryAsync(pythonBatchOutDir, false, cancel: cancel)).Where(x => x.IsFile);
            foreach (var exf in existingFiles)
            {
                await Lfs.DeleteFileIfExistsAsync(exf.FullPath, raiseException: true, cancel: cancel);
            }

            Con.WriteLine($"Real-ESRGAN: Chunk {i + 1} / {srcImgFilesByChunk.Count}: Pre: Copying from '{srcImgDirPath}' to '{pythonBatchInDir}' ...");
            foreach (var src in srcImgFilesOfThisChunk)
            {
                n++;

                string fn = $"{n:D5}" + imgExtension;
                string dstFilePath = PP.Combine(pythonBatchInDir, fn);

                Con.WriteLine($"Real-ESRGAN: Copy '{src.FullPath}' --> '{dstFilePath}'");
                await Lfs.CopyFileAsync(src.FullPath, dstFilePath);
            }
            Con.WriteLine("Real-ESRGAN: Pre: Done.");

            int timeout = (srcImgFilesOfThisChunk.Count() + 10) * 120 * 1000;

            IEnumerable<FileSystemEntity> generatedImgFiles = null!;

            await TaskUtil.RetryAsync(async c =>
            {
                await PerformInternalAsync(option, timeout, cancel);

                generatedImgFiles = (await Lfs.EnumDirectoryAsync(pythonBatchOutDir, false, cancel: cancel)).Where(x => x.IsFile && x.Name._IsExtensionMatch(imgExtension)).OrderBy(x => x.Name, StrCmpi);

                if (generatedImgFiles.Count() != srcImgFilesOfThisChunk.Count())
                {
                    throw new CoresException($"Real-ESRGAN: Post: generatedImgFiles.Count({generatedImgFiles.Count()}) != srcImgFilesOfThisChunk.Count({srcImgFilesOfThisChunk.Count()})");
                }
                return true;
            },
            5, 200, cancel, true);

            Con.WriteLine($"Real-ESRGAN: Chunk {i + 1} / {srcImgFilesByChunk.Count}: Post: Copying from '{pythonBatchOutDir}' to '{dstImgDirPath}' ...");

            foreach (var src in generatedImgFiles)
            {
                string dstFilePath = PP.Combine(dstImgDirPath, src.Name);

                await Lfs.CopyFileAsync(src.FullPath, dstFilePath);
            }
            Con.WriteLine($"Real-ESRGAN: Chunk {i + 1} / {srcImgFilesByChunk.Count}: Post: Done.");
        }
    }

    async Task PerformInternalAsync(AiUtilRealEsrganPerformOption option, int timeout, CancellationToken cancel)
    {
        string additionalStr = "";

        if (option.FaceMode)
        {
            additionalStr += " --face_enhance ";
        }

        if (option.Fp32)
        {
            additionalStr += " --fp32 ";
        }

        var result = await this.RunVEnvPythonCommandsAsync(
            $"python Real-ESRGAN/inference_realesrgan.py -n {option.Model} -i dn_batch_in -o dn_batch_out --tile {option.Tile} --tile_pad {option.Pad} --outscale {option.OutScale:F2} {additionalStr}", timeout, printTag: this.SimpleAiName, cancel: cancel);
    }

    protected override async Task CleanupImplAsync(Exception? ex)
    {
        try
        {
        }
        finally
        {
            await base.CleanupImplAsync(ex);
        }
    }
}

public class AvUtilSeedVcMetaData
{
    public List<MediaVoiceSegment>? VoiceSegments = null;
};

public class AiUtilSeedVcEngine : AiUtilBasicEngine
{
    public FfMpegUtil FfMpeg { get; }
    public bool SingMode { get; }

    public AiUtilSeedVcEngine(AiUtilBasicSettings settings, FfMpegUtil ffMpeg, bool singMode = false) : base(settings, "SEED-VC", settings.AiTest_SeedVc_BaseDir)
    {
        this.SingMode = singMode;
        this.FfMpeg = ffMpeg;
    }

    class SilentRange
    {
        public double StartTime;
        public double Duration;
    }

    public async Task<AvUtilSeedVcMetaData> ConvertAsync(string srcWavPath, string dstWavPath, string voiceSamplePath, int diffusionSteps, bool overwriteSilent, string tagTitle = "", bool useOkFile = true, List<MediaVoiceSegment>? voiceSegments = null, CancellationToken cancel = default)
    {
        if (tagTitle._IsEmpty()) tagTitle = PP.GetFileNameWithoutExtension(srcWavPath);

        string digest = $"voiceSamplePath={voiceSamplePath},diffusionSteps={diffusionSteps},targetMaxVolume={Settings.AdjustAudioTargetMaxVolume},targetMeanVolume={Settings.AdjustAudioTargetMeanVolume}";

        if (useOkFile)
        {
            var metaRet = await Lfs.ReadOkFileAsync<AvUtilSeedVcMetaData>(dstWavPath, digest, AiUtilVersion.CurrentVersion, cancel: cancel);
            if (metaRet.IsOk)
            {
                return metaRet.Value!;
            }
        }

        List<SilentRange> silentRangeList = new();

        if (voiceSegments != null)
        {
            var tmpList = voiceSegments.ToArray().Reverse().ToList();
            int mode = 0;
            for (int i = 0; i < tmpList.Count; i++)
            {
                var cur = tmpList[i];
                var next = tmpList.ElementAtOrDefault(i - 1); // (リスト上は Reverse により逆順になっているので注意)

                if (cur.IsBlank || cur.IsSleep)
                {
                    if (cur.TimeLength >= 0.01)
                    {
                        if (mode == 0)
                        {
                            // 末尾部分
                            silentRangeList.Add(new SilentRange { StartTime = cur.TimePosition, Duration = cur.TimeLength, });
                        }
                        else
                        {
                            if (overwriteSilent)
                            {
                                // 途中の Sleep と、それに続く Blank 部分 (Sleep の直前の Blank 部分は無音化しない)
                                if (cur.IsSleep && next != null && next.IsBlank)
                                {
                                    silentRangeList.Add(new SilentRange { StartTime = cur.TimePosition, Duration = cur.TimeLength + next.TimeLength, });
                                }
                            }
                        }
                    }
                }
                else
                {
                    mode = 1;
                }
            }
        }

        await TaskUtil.RetryAsync(async c =>
        {
            await ConvertInternalAsync(srcWavPath, dstWavPath, voiceSamplePath, diffusionSteps, silentRangeList, tagTitle, cancel);

            return true;
        },
        5, 200, cancel, true);

        var meta = new AvUtilSeedVcMetaData
        {
            VoiceSegments = voiceSegments,
        };

        if (useOkFile)
        {
            await Lfs.WriteOkFileAsync(dstWavPath, meta, digest, AiUtilVersion.CurrentVersion, cancel: cancel);
        }

        return meta;
    }

    async Task ConvertInternalAsync(string srcWavPath, string dstWavPath, string voiceSamplePath, int diffusionSteps, IEnumerable<SilentRange>? silentRanges = null, string tagTitle = "", CancellationToken cancel = default)
    {
        string aiSrcPath = BaseDirPath._CombinePath("test_in_data", "_aiutil_src.wav");

        string aiSamplePath = BaseDirPath._CombinePath("test_in_data", "_aiutil_sample.wav");

        await Lfs.DeleteFileIfExistsAsync(aiSrcPath, cancel: cancel);

        await Lfs.DeleteFileIfExistsAsync(aiSamplePath, cancel: cancel);
        await FfMpeg.AdjustAudioVolumeAsync(voiceSamplePath, aiSamplePath, Settings.AdjustAudioTargetMaxVolume, Settings.AdjustAudioTargetMeanVolume, FfmpegAdjustVolumeOptiono.MeanOnly, voiceSamplePath, false, cancel);

        await Lfs.DeleteFileIfExistsAsync(aiSrcPath, cancel: cancel);
        await FfMpeg.AdjustAudioVolumeAsync(srcWavPath, aiSrcPath, Settings.AdjustAudioTargetMaxVolume, Settings.AdjustAudioTargetMeanVolume, FfmpegAdjustVolumeOptiono.MeanOnly, tagTitle, false, cancel);

        await DeleteAllAiProcessOutputFilesAsync(cancel);

        if (tagTitle._IsEmpty())
        {
            tagTitle = srcWavPath._GetFileNameWithoutExtension();
        }
        string tag = $"Convert ('{tagTitle._TruncStrEx(16)}')";

        int srcWavFileLen = await AiTask.GetWavFileLengthMSecAsync(srcWavPath);
        int timeout = (srcWavFileLen * 2) + (60 * 1000);

        long startTick = TickHighresNow;

        EasyExecResult result;

        if (this.SingMode == false)
        {
            result = await this.RunVEnvPythonCommandsAsync(
                $"python inference.py --source test_in_data/_aiutil_src.wav --target test_in_data/_aiutil_sample.wav " +
                $"--output test_out_dir --diffusion-steps {diffusionSteps} --length-adjust 1.0 --inference-cfg-rate 1.0 --f0-condition False " +
                $"--auto-f0-adjust False --semi-tone-shift 0 --config runs/test02/config_dit_mel_seed_uvit_whisper_small_wavenet.yml " +
                $"--fp16 True", timeout, printTag: tag, cancel: cancel);
        }
        else
        {
            result = await this.RunVEnvPythonCommandsAsync(
                $"python inference.py --source test_in_data/_aiutil_src.wav --target test_in_data/_aiutil_sample.wav " +
                $"--output test_out_dir --diffusion-steps {diffusionSteps} --length-adjust 1.0 --inference-cfg-rate 1.0 --f0-condition True " +
                $"--auto-f0-adjust True --semi-tone-shift 0 --config runs/test02/config_dit_mel_seed_uvit_whisper_base_f0_44k.yml " +
                $"--fp16 True", timeout, printTag: tag, cancel: cancel);
        }

        long endTick = TickHighresNow;
        AiTask.TotalGpuProcessTimeMsecs.Add(endTick - startTick);

        if (result.OutputAndErrorStr._GetLines().Where(x => x.StartsWith("RTF: ")).Any() == false)
        {
            throw new CoresLibException($"{tag} failed.");
        }

        string aiOutDir = PP.Combine(this.BaseDirPath, "test_out_dir");

        var files = await Lfs.EnumDirectoryAsync(aiOutDir, wildcard: "*.wav", cancel: cancel);
        var aiDstFile = files.Single();

        string tmpSrcPath = aiDstFile.FullPath;

        // 出力結果の最後の部分を指定した秒数分無音で埋める (Seed-VC の不具合があり、無音部分からゴーストが発生するため)
        if (silentRanges != null && silentRanges.Any())
        {
            try
            {
                Memory<byte> voiceWavData;
                WaveFormat voiceWavFormat;
                await using (var voiceWavStream = File.Open(aiDstFile.FullPath, FileMode.Open, FileAccess.Read, FileShare.Read))
                {
                    await using var voiceWavReader = new WaveFileReader(voiceWavStream);
                    voiceWavFormat = voiceWavReader.WaveFormat;
                    voiceWavData = await voiceWavStream._ReadToEndAsync();
                }

                double srcLen = AiWaveUtil.CalcWavDurationFromSizeInByte(voiceWavData.Length, voiceWavFormat);

                foreach (var range in silentRanges)
                {
                    if (range.Duration >= 0.1)
                    {
                        int zeroStartInByte = (int)AiWaveUtil.GetWavDataPositionInByteFromTime(range.StartTime, voiceWavFormat);

                        if (zeroStartInByte < voiceWavData.Length)
                        {
                            Memory<byte> silentData = AiWaveUtil.GenerateSilentNoiseData(range.Duration, voiceWavFormat);

                            int silentDataLength = Math.Min(silentData.Length, voiceWavData.Length - zeroStartInByte);

                            silentData = silentData.Slice(0, silentDataLength);

                            silentData.CopyTo(voiceWavData.Slice(zeroStartInByte));
                        }
                    }
                }

                tmpSrcPath = await Lfs.GenerateUniqueTempFilePathAsync("silent", ".wav", cancel: cancel);

                // 結果を wav に書き出す
                await using (var voiceWavTmpPath2Stream = File.Create(tmpSrcPath))
                {
                    await using var writer = new WaveFileWriter(voiceWavTmpPath2Stream, voiceWavFormat);

                    await writer.WriteAsync(voiceWavData, cancellationToken: cancel);
                }
            }
            catch (Exception ex)
            {
                ex._Error();
                throw;
            }
        }

        await Lfs.DeleteFileIfExistsAsync(dstWavPath, cancel: cancel);

        await FfMpeg.AdjustAudioVolumeAsync(tmpSrcPath, dstWavPath, Settings.AdjustAudioTargetMaxVolume, Settings.AdjustAudioTargetMeanVolume, FfmpegAdjustVolumeOptiono.MeanOnly, tagTitle, false, cancel);
    }

    async Task DeleteAllAiProcessOutputFilesAsync(CancellationToken cancel = default)
    {
        string aiOutDir = PP.Combine(this.BaseDirPath, "test_out_dir");

        await Lfs.CreateDirectoryAsync(aiOutDir, cancel: cancel);

        var list = await Lfs.EnumDirectoryAsync(aiOutDir, false, wildcard: "*.wav", cancel: cancel);

        foreach (var f in list)
        {
            await Lfs.DeleteFileIfExistsAsync(f.FullPath, raiseException: true, cancel: cancel);
        }
    }
}

public class AiUtilFishAudioEngine : AiUtilVoiceVoxEngine
{
    public override int BaseWavBitRateKHz => 44100;
    public override int BaseWavBitDepth => 16;
    public override int BaseWavChannels => 2;
    public override int TextSplitMaxLen => 60;

    public AiUtilFishAudioEngine(AiUtilBasicSettings settings, FfMpegUtil ffMpeg) : base(settings, ffMpeg, "FishAudio", settings.AiTest_UvrCli_BaseDir)
    {
    }

    protected override async Task<byte[]> TextBlockToWavAsync(string text, int speakerId, CancellationToken cancel = default)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            throw new ArgumentException("text が null または空です。", nameof(text));
        }

        text = text.Replace("(", " ").Replace(")", " ");

        // Python例[1]と同等の payload を JObject で構築（Newtonsoft.Json）
        // 重要：感情マーカーを入れる場合、正規化が邪魔をすることがあるため normalize=false 推奨（Python例と同様）
        var payload = new JObject
        {
            ["text"] = text,
            ["format"] = "wav",
            ["normalize"] = false,
            ["reference_id"] = speakerId.ToString(),

            // 生成の揺らぎ（Python例と同じ）
            ["temperature"] = 0.8,
            ["top_p"] = 0.9,
            ["repetition_penalty"] = 1.1,

            // 長文向け内部チャンク（100-300程度が目安。Python例と同じ 200）
            ["chunk_length"] = 200,
            ["max_new_tokens"] = 4096,
            ["latency"] = "normal",
        };

        // JSON文字列化（不要な整形はしない）
        string json = payload.ToString(Formatting.None);

        using var requestContent = new StringContent(json, Encoding.UTF8, "application/json");
        using var request = new HttpRequestMessage(HttpMethod.Post, $"http://127.0.0.1:{this.Settings.FishAudioLocalhostPort}/v1/tts")
        {
            Content = requestContent
        };

        using var http = CreateHttpClient();

        // 応答は WAV バイナリ。ヘッダ受領後にストリーム読み込みでも良いが、要件どおり byte[] で返す。
        using HttpResponseMessage response = await http.SendAsync(
            request,
            HttpCompletionOption.ResponseHeadersRead,
            cancel
        ).ConfigureAwait(false);

        if (!response.IsSuccessStatusCode)
        {
            // 失敗時：本文(JSONエラー等)を読んで原因特定しやすくする
            string errorBody = await SafeReadAsStringAsync(response, cancel).ConfigureAwait(false);
            throw new CoresLibException(
                $"TTS API 失敗: {(int)response.StatusCode} {response.ReasonPhrase}. Body={errorBody}"
            );
        }

        byte[] wavBytes = await response.Content.ReadAsByteArrayAsync().ConfigureAwait(false);

        if (wavBytes.Length == 0)
        {
            throw new CoresLibException(
                message: "TTS API の応答ボディが空でした (0 bytes)。"
            );
        }

        // 軽い妥当性チェック：WAVなら先頭が "RIFF" のことが多い（厳密ではないが事故検知に有効）
        if (wavBytes.Length >= 4 &&
            wavBytes[0] == (byte)'R' &&
            wavBytes[1] == (byte)'I' &&
            wavBytes[2] == (byte)'F' &&
            wavBytes[3] == (byte)'F')
        {
            return wavBytes;
        }

        // サーバーが別形式やエラーバイナリを返した可能性もあるので、ここで例外にする
        // ※運用で「とにかく返してほしい」なら、このチェックは外してもよい
        throw new CoresLibException(
            message: "応答がWAV(RIFF)形式に見えません。APIサーバー設定やエンドポイントを確認してください。"
        );
    }


    // テキスト分割 (タグも分離)
    public override KeyValueList<string, bool> SplitText(string text, int maxLen = 100)
    {
        KeyValueList<string, bool> ret = new KeyValueList<string, bool>();

        var textAndTags = SplitTextToNormalAndTag(text);

        foreach (var part in textAndTags)
        {
            if (part.Value == false)
            {
                var lines = part.Key._GetLines(true, singleLineAtLeast: true, trim: true);

                foreach (var line in lines)
                {
                    var a = SplitTextCore(line, maxLen);
                    foreach (var s in a)
                    {
                        ret.Add(s, false);
                    }
                }
            }
            else
            {
                ret.Add(part.Key, true);
            }
        }

        return ret;
    }

    protected override List<string> SplitTextCore(string text, int maxLen = 100)
    {
        var sentences = Regex.Split(text, @"(?<=[。！？、.!?,])");
        var chunks = new List<string>();
        var current = "";

        foreach (var sentence in sentences)
        {
            if (current.Length + sentence.Length > maxLen)
            {
                if (current._IsFilled())
                {
                    chunks.Add(current);
                }
                current = sentence;
            }
            else
            {
                current += sentence;
            }
        }

        if (current._IsFilled())
        {
            chunks.Add(current);
        }

        return chunks;
    }

    private static async Task<string> SafeReadAsStringAsync(HttpResponseMessage response, CancellationToken cancel)
    {
        try
        {
            return await response.Content.ReadAsStringAsync().ConfigureAwait(false);
        }
        catch
        {
            return "<error body を読み取れませんでした>";
        }
    }

    private static HttpClient CreateHttpClient()
    {
        var handler = new SocketsHttpHandler(LocalNet)
        {
            AutomaticDecompression = DecompressionMethods.None
        };

        var client = new HttpClient(handler)
        {
            Timeout = TimeSpan.FromSeconds(600),
        };

        // 応答は WAV バイナリ想定。サーバー側が Content-Type を返さない場合もあるため Accept は補助的。
        client.DefaultRequestHeaders.Accept.Clear();
        client.DefaultRequestHeaders.Accept.Add(new MediaTypeWithQualityHeaderValue("audio/wav"));
        client.DefaultRequestHeaders.Accept.Add(new MediaTypeWithQualityHeaderValue("application/octet-stream"));

        return client;
    }

    protected override async Task<ExecInstance> StartExeAsync(CancellationToken cancel = default)
    {
        var ret = await TaskUtil.RetryAsync(async c =>
        {
            ExecInstance exec = new ExecInstance(new ExecOptions(Settings.AiTest_FishAudio_BaseDir._CombinePath("venv", "bin", "python"),
                $"-m tools.api_server --listen 127.0.0.1:{this.Settings.FishAudioLocalhostPort} --llama-checkpoint-path \"checkpoints\\openaudio-s1-mini\" --decoder-checkpoint-path \"checkpoints\\openaudio-s1-mini\\codec.pth\" --decoder-config-name modded_dac_vq",
                Settings.AiTest_FishAudio_BaseDir));

            Con.WriteLine($"proc id = {exec.ProcessId}");

            try
            {
                await TaskUtil.RetryAsync(async c2 =>
                {
                    await using var http = new WebApi(new WebApiOptions(new WebApiSettings { DoNotThrowHttpResultError = true }, doNotUseTcpStack: true));

                    string url1 = $"http://127.0.0.1:{this.Settings.FishAudioLocalhostPort}/";

                    await http.SimpleQueryAsync(WebMethods.GET, url1, c);

                    return true;
                },
                200, 30, c, true);

                return exec;
            }
            catch
            {
                await exec._DisposeSafeAsync();
                throw;
            }

        },
        1000, 5, cancel, true);

        return ret;
    }
}

public class AiUtilVoiceVoxEngine : AiUtilBasicEngine
{
    public virtual int BaseWavBitRateKHz => 24000;
    public virtual int BaseWavBitDepth => 16;
    public virtual int BaseWavChannels => 1;
    public virtual int TextSplitMaxLen => 100;

    // アルゴリズムの参考元: https://qiita.com/BB-KING777/items/34c3cbb3b4ecc5043a2a BB-KING777
    public FfMpegUtil FfMpeg { get; }

    public AiUtilVoiceVoxEngine(AiUtilBasicSettings settings, FfMpegUtil ffMpeg) : base(settings, "VoiceBox", settings.AiTest_UvrCli_BaseDir)
    {
        this.FfMpeg = ffMpeg;
    }

    public AiUtilVoiceVoxEngine(AiUtilBasicSettings settings, FfMpegUtil ffMpeg, string simpleAiName, string baseDirPath) : base(settings, simpleAiName, baseDirPath)
    {
        this.FfMpeg = ffMpeg;
    }

    public async Task<FfMpegParsedList> TextToWavAsync(string text, IEnumerable<int> speakerIdList /* 0 ～ 98 */, string dstWavPath, string tagTitle, bool useOkFile = true, CancellationToken cancel = default)
    {
        return await TaskUtil.RetryAsync(async c =>
        {
            return await TextToWavMainAsync(text, speakerIdList, dstWavPath, tagTitle, useOkFile, cancel);
        },
        200, 5, cancel, true);
    }

    virtual protected async Task<ExecInstance> StartExeAsync(CancellationToken cancel = default)
    {
        var ret = await TaskUtil.RetryAsync(async c =>
        {
            ExecInstance exec = new ExecInstance(new ExecOptions(Settings.AiTest_VoiceBox_ExePath, Settings.AiTest_VoiceBox_ExeArgs, PP.GetDirectoryName(Settings.AiTest_VoiceBox_ExePath)));
            try
            {
                await TaskUtil.RetryAsync(async c2 =>
                {
                    await using var http = new WebApi(new WebApiOptions(new WebApiSettings { DoNotThrowHttpResultError = true }, doNotUseTcpStack: true));

                    string url1 = $"http://127.0.0.1:{this.Settings.VoiceBoxLocalhostPort}/";

                    await http.SimpleQueryAsync(WebMethods.GET, url1, c);

                    return true;
                },
                200, 5, c, true);

                return exec;
            }
            catch
            {
                await exec._DisposeSafeAsync();
                throw;
            }

        },
        1000, 5, cancel, true);

        return ret;
    }

    async Task<FfMpegParsedList> TextToWavMainAsync(string text, IEnumerable<int> speakerIdList /* 0 ～ 98 */, string dstWavPath, string tagTitle = "", bool useOkFile = true, CancellationToken cancel = default)
    {
        if (tagTitle._IsEmpty()) tagTitle = "voicetext";

        text = PreProcessText(text);

        var textBlockList = SplitText(text, this.TextSplitMaxLen);

        string digest = $"text={textBlockList._ObjectToJson()._Digest()},speakerId={speakerIdList.Select(x => x.ToString())._Combine("+")},targetMaxVolume={Settings.AdjustAudioTargetMaxVolume},targetMeanVolume={Settings.AdjustAudioTargetMeanVolume}";

        if (useOkFile)
        {
            var okResult = await Lfs.ReadOkFileAsync<FfMpegParsedList>(dstWavPath, digest, AiUtilVersion.CurrentVersion, cancel: cancel);
            if (okResult.IsOk && okResult.Value != null) return okResult.Value;
        }

        ShuffledEndlessQueue<int> speakerIdShuffleQueue = new ShuffledEndlessQueue<int>(speakerIdList, 3);

        await using var exec = await StartExeAsync(cancel: cancel);

        List<string> blockWavFileNameList = new List<string>();

        long totalFileSize = 0;

        Con.WriteLine($"{SimpleAiName}: {tagTitle}: Start text to wav");

        List<MediaVoiceSegment> segmentsList = new List<MediaVoiceSegment>();
        Dictionary<int, (MediaVoiceSegment SegmentIndexForVoice, MediaVoiceSegment SegmentIndexForBlank)> wavFileNameIndexToSegmentMappingTable = new();
        HashSetDictionary<int, MediaVoiceSegment> wavFileNameIndexToTagSegmentMappindTable = new();

        for (int i = 0; i < textBlockList.Count; i++)
        {
            if (textBlockList[i].Value == false)
            {
                string block = textBlockList[i].Key;
                int speakerId = speakerIdShuffleQueue.Dequeue();

                byte[] blockWavData = await TextBlockToWavAsync(block, speakerId);

                var tmpPath = await Lfs.GenerateUniqueTempFilePathAsync($"{tagTitle}_{i:D8}_speaker{speakerId:D3}", ".wav", cancel: cancel);

                await Lfs.WriteDataToFileAsync(tmpPath, blockWavData, FileFlags.AutoCreateDirectory, cancel: cancel);

                if (this is AiUtilFishAudioEngine)
                {
                    var tmpPath2 = await Lfs.GenerateUniqueTempFilePathAsync($"{tagTitle}_adj_{i:D8}_speaker{speakerId:D3}", ".wav", cancel: cancel);
                    await this.FfMpeg.AdjustAudioVolumeAsync(tmpPath, tmpPath2, Settings.AdjustAudioTargetMaxVolume, Settings.AdjustAudioTargetMeanVolume, FfmpegAdjustVolumeOptiono.MeanOnly, tagTitle, false, cancel);
                    blockWavFileNameList.Add(tmpPath2);
                }
                else
                {
                    blockWavFileNameList.Add(tmpPath);
                }

                Con.WriteLine($" - {PP.GetFileNameWithoutExtension(tmpPath)} ({blockWavData.Length._ToString3()} bytes)\n  「{block}」");

                int wavFileNameIndex = blockWavFileNameList.Count - 1;

                totalFileSize += blockWavData.LongLength;

                Con.WriteLine($"{SimpleAiName}: {tagTitle}: Text to Wav: {(i + 1)._ToString3()}/{textBlockList.Count._ToString3()}, Size: {totalFileSize._ToString3()} bytes, Speaker: {speakerId:D3}");

                var segmentForVoice = new MediaVoiceSegment
                {
                    VoiceText = block,
                    SpeakerId = speakerId,
                };

                var segmentForBlank = new MediaVoiceSegment
                {
                    IsBlank = true,
                    SpeakerId = speakerId,
                    BlankDuration = Util.GenRandInterval((0.75)._ToTimeSpanSecs()).TotalSeconds, // 0.75 秒 +/- 30%
                };

                wavFileNameIndexToSegmentMappingTable[wavFileNameIndex] = new(segmentForVoice, segmentForBlank);

                segmentsList.Add(segmentForVoice);

                segmentsList.Add(segmentForBlank);
            }
            else if (textBlockList[i].Key.StartsWith("<SLEEP:", StrCmp) || textBlockList[i].Key.StartsWith("<SLEEP_", StrCmp))
            {
                // 特別タグ: <SLEEP:xxx> または <SLEEP_xxx>
                string innerText = textBlockList[i].Key._RemoveQuotation('<', '>');
                if (innerText._IsFilled())
                {
                    string sepstr = ":";

                    if (innerText.StartsWith("SLEEP_", StrCmp))
                    {
                        sepstr = "_";
                    }

                    if (innerText._GetKeyAndValue(out var sleepTagStr, out var durationStr, sepstr))
                    {
                        double durationOriginal = Math.Min(durationStr._ToDouble(), 3600);
                        double duration = Math.Max(durationOriginal - 0.1, 0.11);
                        WaveFormat waveFormat = new WaveFormat(this.BaseWavBitRateKHz, this.BaseWavBitDepth, this.BaseWavChannels);

                        int speakerId = 0;
                        int silenceBytes = AiWaveUtil.GetWavDataSizeInByteFromTime(duration, waveFormat);
                        byte[] blockWavData = new byte[silenceBytes];

                        var tmpPath = await Lfs.GenerateUniqueTempFilePathAsync($"{tagTitle}_{i:D8}_speaker{speakerId:D3}", ".wav", cancel: cancel);

                        MemoryStream waveMs = new();
                        await using (WaveFileWriter writer = new(waveMs, waveFormat))
                        {
                            await writer.WriteAsync(blockWavData, cancel);
                            await writer.FlushAsync();
                        }

                        await Lfs.WriteDataToFileAsync(tmpPath, waveMs.ToArray(), FileFlags.AutoCreateDirectory, cancel: cancel);

                        blockWavFileNameList.Add(tmpPath);
                        int wavFileNameIndex = blockWavFileNameList.Count - 1;

                        totalFileSize += blockWavData.LongLength;

                        Con.WriteLine($"{SimpleAiName}: {tagTitle}: Text to Wav (Sleep): {(i + 1)._ToString3()}/{textBlockList.Count._ToString3()}, Size: {totalFileSize._ToString3()} bytes, Speaker: {speakerId:D3}");

                        var segmentForVoice = new MediaVoiceSegment
                        {
                            VoiceText = "",
                            SpeakerId = speakerId,
                            IsSleep = true,
                            SleepDuration = duration,
                        };

                        var segmentForBlank = new MediaVoiceSegment
                        {
                            IsBlank = true,
                            SpeakerId = speakerId,
                            BlankDuration = 0.1,
                        };

                        wavFileNameIndexToSegmentMappingTable[wavFileNameIndex] = new(segmentForVoice, segmentForBlank);

                        segmentsList.Add(segmentForVoice);

                        segmentsList.Add(segmentForBlank);
                    }
                }
            }
            else
            {
                var segmentForTag = new MediaVoiceSegment
                {
                    IsTag = true,
                    TagStr = textBlockList[i].Key,
                };

                segmentsList.Add(segmentForTag);

                int wavFileNameIndex = blockWavFileNameList.Count;

                wavFileNameIndexToTagSegmentMappindTable.Add(wavFileNameIndex, segmentForTag);
            }
        }

        Con.WriteLine($"{SimpleAiName}: {tagTitle}: Combining...");

        string concatFile = await Lfs.GenerateUniqueTempFilePathAsync($"{tagTitle}_concat", ".wav", cancel: cancel);

        if (blockWavFileNameList.Any() == false)
        {
            WaveFormat waveFormat = new WaveFormat(this.BaseWavBitRateKHz, this.BaseWavBitDepth, this.BaseWavChannels);

            await using var writer = new WaveFileWriter(concatFile, waveFormat);

            int bytesPerSample = waveFormat.BitsPerSample / 8;
            int bytesPerSecond = waveFormat.SampleRate * waveFormat.Channels * bytesPerSample;
            double silenceDurationSeconds = 2.0; // 2.0 秒
            int silenceBytes = (int)(bytesPerSecond * silenceDurationSeconds);

            var silenceBuffer = new byte[silenceBytes];
            writer.Write(silenceBuffer, 0, silenceBuffer.Length);
        }
        else
        {
            WaveFormat? waveFormat = null;
            using (var reader = new WaveFileReader(blockWavFileNameList.First()))
            {
                waveFormat = reader.WaveFormat;
            }

            if (waveFormat == null)
            {
                throw new CoresLibException($"{SimpleAiName}: {tagTitle}: WaveFormat not found");
            }

            await using var writer = new WaveFileWriter(concatFile, waveFormat);

            for (int i = 0; i < blockWavFileNameList.Count; i++)
            {
                string srcFileName = blockWavFileNameList[i];

                await using (var reader = new WaveFileReader(srcFileName))
                {
                    var segmentForVoice = wavFileNameIndexToSegmentMappingTable[i].SegmentIndexForVoice;
                    var segmentForBlank = wavFileNameIndexToSegmentMappingTable[i].SegmentIndexForBlank;

                    segmentForVoice.DataPosition = writer.Position;

                    var buffer = new byte[reader.WaveFormat.AverageBytesPerSecond * 4];
                    int bytesRead;
                    while ((bytesRead = reader.Read(buffer, 0, buffer.Length)) > 0)
                    {
                        writer.Write(buffer, 0, bytesRead);
                    }

                    segmentForVoice.DataLength = writer.Position - segmentForVoice.DataPosition;


                    int bytesPerSample = waveFormat.BitsPerSample / 8;
                    int bytesPerSecond = waveFormat.SampleRate * waveFormat.Channels * bytesPerSample;

                    segmentForVoice.TimePosition = (double)segmentForVoice.DataPosition / (double)bytesPerSecond;
                    segmentForVoice.TimeLength = (double)segmentForVoice.DataLength / (double)bytesPerSecond;


                    if (wavFileNameIndexToTagSegmentMappindTable.TryGetValue(i, out var tagSegments))
                    {
                        foreach (var seg in tagSegments)
                        {
                            seg.DataPosition = segmentForVoice.DataPosition;
                            seg.TimePosition = segmentForVoice.TimePosition;
                        }
                    }

                    double silenceDurationSeconds = segmentForBlank.BlankDuration;
                    if (silenceDurationSeconds < 0.00001)
                    {
                        silenceDurationSeconds = 0.5; // 未設定の場合は 0.5 秒
                    }

                    int silenceBytes = AiWaveUtil.GetWavDataSizeInByteFromTime(silenceDurationSeconds, waveFormat);

                    var silenceBuffer = new byte[silenceBytes];

                    segmentForBlank.DataPosition = writer.Position;

                    writer.Write(silenceBuffer, 0, silenceBuffer.Length);

                    segmentForBlank.DataLength = writer.Position - segmentForBlank.DataPosition;

                    segmentForBlank.TimePosition = (double)segmentForBlank.DataPosition / (double)bytesPerSecond;
                    segmentForBlank.TimeLength = (double)segmentForBlank.DataLength / (double)bytesPerSecond;
                }

                Con.WriteLine($"{SimpleAiName}: {tagTitle}: Concat wav: {(i + 1)._ToString3()}/{textBlockList.Count._ToString3()}, Size: {writer.Length._ToString3()} bytes");
            }
        }

        var results = await this.FfMpeg.AdjustAudioVolumeAsync(concatFile, dstWavPath, Settings.AdjustAudioTargetMaxVolume, Settings.AdjustAudioTargetMeanVolume, FfmpegAdjustVolumeOptiono.MeanOnly, tagTitle, false, cancel);

        results.Item2.Options_VoiceSegmentsList = segmentsList;

        if (useOkFile)
        {
            await Lfs.WriteOkFileAsync(dstWavPath, results.Item2, digest, AiUtilVersion.CurrentVersion, cancel);
        }

        return results.Item2;
    }

    // 音声合成
    protected virtual async Task<byte[]> TextBlockToWavAsync(string text, int speakerId, CancellationToken cancel = default)
    {
        await using var http = new WebApi(new WebApiOptions(doNotUseTcpStack: true));

        string url1 = $"http://127.0.0.1:{this.Settings.VoiceBoxLocalhostPort}/audio_query?text={Uri.EscapeDataString(text)}&speaker={speakerId}";

        var result1 = await http.SimplePostDataAsync(url1, new byte[0], cancel);

        string queryJsonStr = result1.ToString();

        string url2 = $"http://127.0.0.1:{this.Settings.VoiceBoxLocalhostPort}/synthesis?speaker={speakerId}";

        long startTick = TickHighresNow;

        var result2 = await http.SimplePostDataAsync(url2, queryJsonStr._GetBytes_UTF8(), cancel, "application/json");

        long endTick = TickHighresNow;
        AiTask.TotalGpuProcessTimeMsecs.Add(endTick - startTick);

        return result2.Data;
    }

    // テキスト文字列からタグとそれ以外を分離
    public static KeyValueList<string, bool> SplitTextToNormalAndTag(string text)
    {
        // "SLEEP:" の前に必ず改行を入れる
        StringBuilder tmpBuilder = new();
        string[] tmpTokens = text._Split(StringSplitOptions.None, "SLEEP:");
        for (int k = 0; k < tmpTokens.Length; k++)
        {
            string pref = tmpTokens.ElementAtOrDefault(k - 1)._NonNull();
            string cur = tmpTokens.ElementAtOrDefault(k)._NonNull();

            if (k >= 1)
            {
                if (pref.EndsWith("<") == false)
                {
                    tmpBuilder.AppendLine();
                }
                tmpBuilder.Append("SLEEP:");
            }
            tmpBuilder.Append(cur);
        }
        text = tmpBuilder.ToString();

        // 前処理: 1 行特別タグ ( < > なし) の変換
        var lines = text._GetLines();
        StringWriter w = new();
        foreach (var line in lines)
        {
            // EOF 以降を無視
            if (line._InStri("EOAI_DATA") || line._InStri("EOAI_TITLE") || line._InStri("EOAI_METADATA") || line._IsSameiTrim("[EOF]"))
            {
                break;
            }

            // "SLEEP:xxx" タグ -> "<SLEEP:xxx>" タグ
            string line2 = line;
            if (line.StartsWith("SLEEP:", StrCmp))
            {
                StringBuilder sleepStrBuilder = new();
                StringBuilder restStrBuilder = new();
                int mode2 = 0;
                foreach (char c in line)
                {
                    if (mode2 == 0)
                    {
                        if (c._IsAscii() && c != ' ' && c != '\t')
                        {
                            sleepStrBuilder.Append(c);
                        }
                        else
                        {
                            mode2 = 1;
                        }
                    }

                    if (mode2 == 1)
                    {
                        restStrBuilder.Append(c);
                    }
                }

                line2 = $"<{sleepStrBuilder.ToString()}> {restStrBuilder.ToString()}";
            }
            w.WriteLine(line2);
        }

        text = w.ToString();

        StringBuilder b = new StringBuilder();

        KeyValueList<string, bool> ret = new();

        int mode = 0;

        int i;
        for (i = 0; i < text.Length; i++)
        {
            char c = text[i];

            if (c == '<')
            {
                if (mode == 0)
                {
                    mode = 1;

                    if (b.Length >= 1)
                    {
                        ret.Add(b.ToString(), false);
                        b.Clear();
                    }
                }

                b.Append(c);
            }
            else if (c == '>')
            {
                b.Append(c);

                if (mode == 1)
                {
                    mode = 0;

                    ret.Add(b.ToString(), true);
                    b.Clear();
                }
            }
            else
            {
                b.Append(c);
            }
        }

        if (b.Length >= 1)
        {
            ret.Add(b.ToString(), mode != 0);
        }

        return ret;
    }

    // テキスト分割 (タグも分離)
    public virtual KeyValueList<string, bool> SplitText(string text, int maxLen = 100)
    {
        KeyValueList<string, bool> ret = new KeyValueList<string, bool>();

        var textAndTags = SplitTextToNormalAndTag(text);

        foreach (var part in textAndTags)
        {
            if (part.Value == false)
            {
                var a = SplitTextCore(part.Key, maxLen);
                foreach (var s in a)
                {
                    ret.Add(s, false);
                }
            }
            else
            {
                ret.Add(part.Key, true);
            }
        }

        return ret;
    }

    // テキスト分割
    protected virtual List<string> SplitTextCore(string text, int maxLen = 100)
    {
        var sentences = Regex.Split(text, @"(?<=[。！？])");
        var chunks = new List<string>();
        var current = "";

        foreach (var sentence in sentences)
        {
            if (current.Length + sentence.Length > maxLen)
            {
                if (current._IsFilled())
                {
                    chunks.Add(current);
                }
                current = sentence;
            }
            else
            {
                current += sentence;
            }
        }

        if (current._IsFilled())
        {
            chunks.Add(current);
        }

        return chunks;
    }

    // テキスト前処理
    static string PreProcessText(string text)
    {
        text = text._ReplaceStr("　", " ")._ReplaceStr("\t", " ");

        return text;
    }
}

public class AiUtilUvrEngine : AiUtilBasicEngine
{
    public FfMpegUtil FfMpeg { get; }

    public AiUtilUvrEngine(AiUtilBasicSettings settings, FfMpegUtil ffMpeg) : base(settings, "UVR", settings.AiTest_UvrCli_BaseDir)
    {
        this.FfMpeg = ffMpeg;
    }

    public async Task<FfMpegParsedList> ExtractAsync(string srcFilePath, string? dstMusicWavPath = null, string? dstVocalWavPath = null, string tagTitle = "", bool useOkFile = true, CancellationToken cancel = default)
    {
        if (tagTitle._IsEmpty()) tagTitle = PP.GetFileNameWithoutExtension(srcFilePath);

        if (dstMusicWavPath._IsEmpty() && dstVocalWavPath._IsEmpty()) throw new CoresLibException("dstMusicWavPath and dstVocalWavPath are both empty.");

        if (useOkFile)
        {
            FfMpegParsedList? savedResult = null;
            if (dstMusicWavPath._IsFilled())
            {
                var okFileForDstMusicWavPath = await Lfs.ReadOkFileAsync<FfMpegParsedList>(dstMusicWavPath, "", AiUtilVersion.CurrentVersion, cancel: cancel);
                if (okFileForDstMusicWavPath.IsOk)
                {
                    dstMusicWavPath = null;
                    savedResult = okFileForDstMusicWavPath;
                }
            }

            if (dstVocalWavPath._IsFilled())
            {
                var okFileForDstVocalWavPath = await Lfs.ReadOkFileAsync<FfMpegParsedList>(dstVocalWavPath, "", AiUtilVersion.CurrentVersion, cancel: cancel);
                if (okFileForDstVocalWavPath.IsOk)
                {
                    dstVocalWavPath = null;
                    savedResult = okFileForDstVocalWavPath;
                }
            }

            if (dstMusicWavPath == null && dstVocalWavPath == null && savedResult != null)
            {
                return savedResult;
            }
        }

        // 音量調整
        string adjustedWavFile = await Lfs.GenerateUniqueTempFilePathAsync(srcFilePath, cancel: cancel);
        var result = await FfMpeg.AdjustAudioVolumeAsync(srcFilePath, adjustedWavFile, Settings.AdjustAudioTargetMaxVolume, Settings.AdjustAudioTargetMeanVolume, FfmpegAdjustVolumeOptiono.MeanOnly /* ! */, tagTitle, false, cancel);

        if (dstMusicWavPath._IsFilled())
        {
            // 音楽分離実行
            await ExtractInternalAsync(adjustedWavFile, dstMusicWavPath, true, tagTitle, cancel);

            if (useOkFile)
            {
                await Lfs.WriteOkFileAsync(dstMusicWavPath, result.Item1, "", AiUtilVersion.CurrentVersion, cancel);
            }
        }

        if (dstVocalWavPath._IsFilled())
        {
            // ボーカル分離実行
            await ExtractInternalAsync(adjustedWavFile, dstVocalWavPath, false, tagTitle, cancel);

            if (useOkFile)
            {
                await Lfs.WriteOkFileAsync(dstVocalWavPath, result.Item1, "", AiUtilVersion.CurrentVersion, cancel);
            }
        }

        return result.Item1;
    }

    async Task ExtractInternalAsync(string srcWavPath, string dstWavPath, bool music, string tagTitle = "", CancellationToken cancel = default)
    {
        string aiSrcPath = BaseDirPath._CombinePath("_src.wav");

        await Lfs.DeleteFileIfExistsAsync(aiSrcPath, cancel: cancel);

        await Lfs.CopyFileAsync(srcWavPath, aiSrcPath, new CopyFileParams(flags: FileFlags.AutoCreateDirectory), cancel: cancel);

        await DeleteAllAiProcessOutputFilesAsync(cancel);

        if (tagTitle._IsEmpty())
        {
            tagTitle = srcWavPath._GetFileNameWithoutExtension();
        }
        string tag = $"{(music ? "get_music" : "get_vocal")} ('{tagTitle._TruncStrEx(16)}')";

        int srcWavFileLen = await AiTask.GetWavFileLengthMSecAsync(srcWavPath);
        int timeout = (srcWavFileLen * 2) + (60 * 1000);

        long startTick = TickHighresNow;
        var result = await this.RunVEnvPythonCommandsAsync($"python {(music ? "get_music" : "get_vocal")}_wav.py", timeout, printTag: tag, cancel: cancel);
        long endTick = TickHighresNow;
        AiTask.TotalGpuProcessTimeMsecs.Add(endTick - startTick);

        if (result.OutputAndErrorStr._GetLines().Where(x => x._InStr("instruments done")).Any() == false)
        {
            throw new CoresLibException($"{tag} failed.");
        }

        string aiOutDir = PP.Combine(this.BaseDirPath, "opt");

        var files = await Lfs.EnumDirectoryAsync(aiOutDir, wildcard: "instrument*.wav", cancel: cancel);
        var aiDstFile = files.Single();

        await Lfs.CopyFileAsync(aiDstFile.FullPath, dstWavPath, new CopyFileParams(flags: FileFlags.AutoCreateDirectory), cancel: cancel);
    }

    async Task DeleteAllAiProcessOutputFilesAsync(CancellationToken cancel = default)
    {
        string aiOutDir = PP.Combine(this.BaseDirPath, "opt");

        await Lfs.CreateDirectoryAsync(aiOutDir, cancel: cancel);

        var list = await Lfs.EnumDirectoryAsync(aiOutDir, false, wildcard: "*.wav", cancel: cancel);

        foreach (var f in list)
        {
            await Lfs.DeleteFileIfExistsAsync(f.FullPath, raiseException: true, cancel: cancel);
        }
    }
}

public class AiUtilBasicEngine : AsyncService
{
    public AiUtilBasicSettings Settings { get; }
    public string SimpleAiName { get; }
    public string BaseDirPath { get; }

    public AiUtilBasicEngine(AiUtilBasicSettings settings, string simpleAiName, string baseDirPath)
    {
        this.SimpleAiName = simpleAiName._NonNullTrim()._NotEmptyOrDefault("AI");
        this.Settings = settings;
        this.BaseDirPath = baseDirPath;
    }

    public async Task<EasyExecResult> RunVEnvPythonCommandsAsync(string commandLines,
        int timeout = Timeout.Infinite, bool throwOnErrorExitCode = true, string printTag = "",
        int easyOutputMaxSize = 0,
        Encoding? inputEncoding = null, Encoding? outputEncoding = null, Encoding? errorEncoding = null,
        CancellationToken cancel = default)
    {
        return await RunBatchCommandsDirectAsync(
            BuildLines("source ./venv/bin/activate",
            commandLines),
            timeout,
            throwOnErrorExitCode,
            printTag,
            easyOutputMaxSize,
            inputEncoding,
            outputEncoding,
            errorEncoding,
            cancel);
    }

    public async Task<EasyExecResult> RunBatchCommandsDirectAsync(string commandLines,
        int timeout = Timeout.Infinite, bool throwOnErrorExitCode = true, string printTag = "",
        int easyOutputMaxSize = 0,
        Encoding? inputEncoding = null, Encoding? outputEncoding = null, Encoding? errorEncoding = null,
        CancellationToken cancel = default)
    {
        if (easyOutputMaxSize <= 0) easyOutputMaxSize = CoresConfig.DefaultAiUtilSettings.DefaultMaxStdOutBufferSize;

        string win32cmd = "/bin/bash";

        commandLines = BuildLines(commandLines, "exit");

        string tmp1 = "";
        if (printTag._IsFilled())
        {
            tmp1 += ": " + printTag.Trim();
        }
        string printTagMain = $"[{this.SimpleAiName}{tmp1}]";

        EasyExecResult ret = await EasyExec.ExecAsync(win32cmd, "", this.BaseDirPath,
            easyInputStr: commandLines,
            flags: ExecFlags.Default | ExecFlags.EasyPrintRealtimeStdOut | ExecFlags.EasyPrintRealtimeStdErr,
            timeout: timeout, cancel: cancel, throwOnErrorExitCode: true,
            easyOutputMaxSize: easyOutputMaxSize,
            printTag: printTagMain,
            inputEncoding: inputEncoding, outputEncoding: outputEncoding, errorEncoding: errorEncoding);

        return ret;
    }

    protected override async Task CleanupImplAsync(Exception? ex)
    {
        try
        {
        }
        finally
        {
            await base.CleanupImplAsync(ex);
        }
    }
}

public class AiRandomBgpReplaceRanges
{
    public double StartPosition;
    public double Length;
    public double FadeIn;
    public double FadeOut;
    public double Margin;
}

public class AiVoiceSettingFactory
{
    public Func<string, AiVoiceSettings> GetAiVoiceSetting = null!;
}

public class AiVoiceSettings
{
    public string SrcSampleVoiceFileNameOrRandDir = "";
    public string SpeakerIdStrOrListFilePath = "";
    public bool MixedMode = false;
    public int DiffusionSteps = 50;
    public bool OverwriteSilent = false;
    public AiTaskTtsEngine TtsEngine = AiTaskTtsEngine.VoiceVox;
    public KeyValueList<string, string>? ReplaceStrList = new KeyValueList<string, string>();
}

public class AiRandomBgmSettingsFactory
{
    public Func<string, object?, KeyValueList<string, AiRandomBgmSettings>> GetBgmSettingListProc = null!;
}

public class AiRandomBgmSettings
{
    public bool Medley = false;
    public bool SmoothMode = false;
    public int TailFadeoutSecs = 10;
    public double BgmVolumeDeltaForConstant = -9.3;
    public double BgmVolumeDeltaForSmooth = -0.0;

    public bool Concat_UseTailIfSingle = false;

    public int Medley_SingleFilePartMSecs = 100 * 1000;
    public int Medley_FadeInOutMsecs = 5 * 1000;
    public int Medley_PlusMinusPercentage = 44;
    public int Medley_MarginMsecs = 15 * 1000;

    public string SrcBgmDirOrFilePath = "";

    public string ReplaceBgmDirPath = "";

    [JsonIgnore]
    public AiCompositWaveSettings CompositWaveSettings = null!;

    [JsonIgnore]
    public Func<CancellationToken, AiAudioEffectFilter?>? CreateAudioEffectFilterForNormalMusicPartProc = null!;

    [JsonIgnore]
    public Func<CancellationToken, AiAudioEffectFilter?>? CreateAudioEffectFilterForOverwriteMusicPartProc = null!;
}













// ChatGPT 5 Pro で生成


// ルート名前空間直下に定義するクラス
public static class AiMixTwoMusicsLib2
{
    // WAV フォーマット (問題仕様に依存)
    const int SampleRate = 44100;
    const int Channels = 2;
    const int BitsPerSample = 16;
    const int BlockAlign = Channels * BitsPerSample / 8; // 1 フレームあたりのバイト数
    const double SamplesPerMs = SampleRate / 1000.0;

    // 乱数生成 (パート長や開始位置のランダム化に使用)
    static readonly Random s_rand = new Random();

    // 1 つの元楽曲 (sourceWav) を表現するための内部クラス
    sealed class SourceSong
    {
        public short[] Samples = Array.Empty<short>(); // 16bit PCM (LRLR...) の配列
        public int FrameCount;                         // フレーム数 (ステレオ 1 フレーム = 2 サンプル)
        public double DurationMs;                      // 長さ (ミリ秒)
    }

    // 出力用に切り出した 1 パート (ア / イ / ウ) を表現
    sealed class PartSegment
    {
        public SourceSong Song = default!;
        public int StartFrame;     // 元楽曲内のフレーム開始位置
        public int FrameLength;    // 使用するフレーム数
        public bool IsFirst;       // 先頭パート (ア) かどうか
        public bool IsLast;        // 末尾パート (ウ) かどうか
    }

    /// <summary>
    /// 問題文で指定されたメイン関数。
    /// 複数の WAV を指定の長さでメドレー風にミックスし、新しい WAV バイト列を返す。
    /// </summary>
    public static byte[] MixTwoMusicsMultiple(
        IEnumerable<ReadOnlyMemory<byte>> sourceWavList,
        int numPartsMeyasu, int targetLengthMsecs, int mixFadeMsecs)
    {
        // ********** 引数チェック **********

        if (sourceWavList == null) throw new ArgumentNullException(nameof(sourceWavList));

        var wavArray = sourceWavList.ToArray();
        if (wavArray.Length == 0)
            throw new ArgumentException("sourceWavList は 1 個以上の要素を含む必要があります。", nameof(sourceWavList));

        if (targetLengthMsecs <= 0)
            throw new ArgumentOutOfRangeException(nameof(targetLengthMsecs), "targetLengthMsecs は正の値である必要があります。");

        if (mixFadeMsecs < 0)
            throw new ArgumentOutOfRangeException(nameof(mixFadeMsecs), "mixFadeMsecs は負の値にはできません。");

        // numPartsMeyasu が 2 未満の場合は 2 とみなす
        if (numPartsMeyasu < 2) numPartsMeyasu = 2;

        // ********** 入力 WAV をパースして内部表現に変換 **********

        var sources = new List<SourceSong>();

        foreach (var mem in wavArray)
        {
            // ReadOnlyMemory<byte> から MemoryStream を作成
            // ※ ToArray() により 1 回コピーが発生するが、分かりやすさ優先
            using var ms = new MemoryStream(mem.ToArray(), writable: false);
            using var reader = new WaveFileReader(ms);

            var wf = reader.WaveFormat;

            // フォーマットチェック (PCM / 44.1kHz / 16bit / stereo)
            if (wf.Encoding != WaveFormatEncoding.Pcm ||
                wf.SampleRate != SampleRate ||
                wf.BitsPerSample != BitsPerSample ||
                wf.Channels != Channels)
            {
                throw new InvalidOperationException(
                    $"WAV フォーマットが不正です。PCM / 44100Hz / 16bit / ステレオ のみ対応しています。 " +
                    $"Encoding={wf.Encoding}, SampleRate={wf.SampleRate}, Bits={wf.BitsPerSample}, Channels={wf.Channels}");
            }

            if (reader.Length <= 0)
                throw new InvalidOperationException("WAV データにサンプルが含まれていません。");

            int dataBytes = (int)reader.Length; // WaveFileReader.Length はデータ部の長さ
            var pcmBytes = new byte[dataBytes];
            int read = reader.Read(pcmBytes, 0, dataBytes);
            if (read != dataBytes)
                throw new InvalidOperationException("WAV データの読み込みに失敗しました。");

            // 16bit PCM (little endian) を short 配列に変換
            var samples = new short[dataBytes / 2];
            Buffer.BlockCopy(pcmBytes, 0, samples, 0, dataBytes);

            int frameCount = dataBytes / BlockAlign; // 1 フレーム = 4 バイト (16bit stereo)
            double durationMs = frameCount * 1000.0 / SampleRate;

            sources.Add(new SourceSong
            {
                Samples = samples,
                FrameCount = frameCount,
                DurationMs = durationMs,
            });
        }

        double avgLenMs = sources.Average(s => s.DurationMs);
        double minLenMs = sources.Min(s => s.DurationMs);

        // ********** 基準パート長 (ミリ秒) の算出 **********
        // (a) targetLengthMsecs / numPartsMeyasu
        // (b) 全曲の平均長さの 1/2
        // (c) 最短曲の 70% (ただし (b)/2 以上)
        double candA = (double)targetLengthMsecs / numPartsMeyasu;
        double candB = avgLenMs / 2.0;
        double candCtmp = minLenMs * 0.7;
        double candBhalf = candB / 2.0;
        double candC = Math.Max(candCtmp, candBhalf);
        double basePartLenMs = Math.Min(candA, Math.Min(candB, candC));

        // 何らかの理由で 0 に近くなった場合の保険
        if (basePartLenMs <= 10.0)
        {
            // 最低 10ms か、target の 1/numParts 程度を確保
            basePartLenMs = Math.Min(targetLengthMsecs, Math.Max(10.0, minLenMs * 0.3));
        }

        // ミリ秒 → フレーム数への変換
        int basePartLenSamples = (int)Math.Round(basePartLenMs * SamplesPerMs);
        int minPartLenSamples = (int)Math.Round(basePartLenSamples * 0.7); // 70%
        int maxPartLenSamples = basePartLenSamples;                       // 100% 相当

        if (minPartLenSamples < 1) minPartLenSamples = 1;

        // ********** フェード長 (ミリ秒→サンプル) の補正 **********
        int fadeSamples = 0;
        if (mixFadeMsecs > 0 && minPartLenSamples > 0)
        {
            int requestedFadeSamples = (int)Math.Round(mixFadeMsecs * SamplesPerMs);
            // 「ア、イ、ウの長さのうち最小値」の 1/3 以下となるよう補正
            int maxFadeSamples = Math.Max(1, minPartLenSamples / 3);
            fadeSamples = Math.Min(requestedFadeSamples, maxFadeSamples);
        }

        // 出力のターゲット長 (フレーム数)
        int targetFrames = (int)Math.Round(targetLengthMsecs * SamplesPerMs);
        if (targetFrames <= 0)
            throw new ArgumentException("targetLengthMsecs が小さすぎます。", nameof(targetLengthMsecs));

        // ********** パート列 (ア + 0 個以上のイ + ウ) の構築 **********

        // 乱数ヘルパー (maxInclusive を含む)
        int NextRandom(int minInclusive, int maxInclusive)
        {
            lock (s_rand)
            {
                return s_rand.Next(minInclusive, maxInclusive + 1);
            }
        }

        // sourceWavList の順序を無限にシャッフルしながら取り出すキュー
        var queue = new ShuffledEndlessQueue<SourceSong>(sources, avoidLastSame: true);

        var segments = new List<PartSegment>();

        // --- 先頭パート (ア) の生成 ---
        {
            SourceSong song = null!;
            for (int i = 0; i < sourceWavList.Count() * 2; i++)
            {
                // 0 個目が出るまで
                song = queue.Dequeue();
                if (song == sources[0])
                {
                    break;
                }
            }
            int maxEndFrame = (int)(song.FrameCount * 0.8); // 80% 位置までで終わる必要あり
            if (maxEndFrame <= 0) maxEndFrame = song.FrameCount;

            // いったん最大パート長をベースにする
            int lenFramesCandidate = Math.Min(maxPartLenSamples, maxEndFrame);
            if (lenFramesCandidate < minPartLenSamples)
            {
                // 80% までで長さが足りなければ、取りうる最大長で妥協する
                lenFramesCandidate = maxEndFrame;
            }

            if (lenFramesCandidate < 1)
                lenFramesCandidate = Math.Min(song.FrameCount, targetFrames);

            // 基準パート長の 70% ～ 100% の乱数
            int lenFrames = NextRandom(
                Math.Max(1, Math.Min(minPartLenSamples, lenFramesCandidate)),
                Math.Max(1, lenFramesCandidate)
            );

            // targetFrames を超えないようにさらに制限
            if (lenFrames > targetFrames)
                lenFrames = targetFrames;

            segments.Add(new PartSegment
            {
                Song = song,
                StartFrame = 0,        // 「先頭部分」で始まる
                FrameLength = lenFrames,
                IsFirst = true,
                IsLast = false,
            });
        }

        // 現在までの出力長 (フレーム数) を、crossfade の実効長を考慮して管理する
        // 先頭パートはのりしろ無しなのでそのまま加算
        int totalFramesSoFar = segments[0].FrameLength;

        // --- 中間パート (イ) の生成 ---
        while (true)
        {
            int remainingFrames = targetFrames - totalFramesSoFar;
            if (remainingFrames <= 0)
            {
                // 既に必要長を達成 or 超過しているので、これ以上イは追加しない
                break;
            }

            // 末尾パート (ウ) 用に最低限確保しておきたい長さ (かなり保守的に設定)
            int minFinalFrames = Math.Max(minPartLenSamples, fadeSamples * 2);
            int minFinalContribution = Math.Max(1, minFinalFrames - fadeSamples);

            // もし残りが最小の末尾パート分より小さければ、イを追加せずにウを作る
            int remainingContribution = remainingFrames;
            if (remainingContribution <= minFinalContribution)
            {
                break;
            }

            // ウに割り当てるべき時間を確保しながらイを追加できるかを簡易的に判定
            int maxFinalContribution = Math.Max(minFinalFrames, maxPartLenSamples) - fadeSamples;
            if (remainingContribution - minFinalContribution <= maxFinalContribution)
            {
                // これ以上イを足すとウが短くなりすぎる可能性が高いのでここで停止
                break;
            }

            var song = queue.Dequeue();

            int totalFrames = song.FrameCount;
            int minStartFrame = (int)(totalFrames * 0.2); // 開始位置は 20% より後
            int maxEndFrame2 = (int)(totalFrames * 0.8);  // 終了位置は 80% より前
            if (maxEndFrame2 <= minStartFrame + 10)
            {
                // 極端に短くて条件を満たせない場合は、今回は中間パートとしては使わずスキップ
                continue;
            }

            int maxLenBySong = maxEndFrame2 - minStartFrame;
            int maxLenForThis = Math.Min(maxPartLenSamples, maxLenBySong);

            // 残りとウのための最低長を考慮して、今回のイの最大長を制限
            int maxNetContributionForThis = remainingContribution - minFinalContribution;
            int maxLenByRemaining = maxNetContributionForThis + fadeSamples;
            if (maxLenByRemaining < maxLenForThis)
                maxLenForThis = maxLenByRemaining;

            if (maxLenForThis <= fadeSamples + 1)
            {
                // もはやイを追加する余裕がない
                break;
            }

            int minLenForThis = Math.Min(minPartLenSamples, maxLenForThis);
            if (minLenForThis < fadeSamples + 1)
                minLenForThis = fadeSamples + 1;

            int desiredLen = NextRandom(minLenForThis, maxLenForThis);

            // 開始位置をランダムに決定 (20% ～ (80% - desiredLen) の範囲)
            int startMax = maxEndFrame2 - desiredLen;
            if (startMax <= minStartFrame)
            {
                // ほとんど余裕がない場合は開始位置を 20% に固定
                minStartFrame = Math.Min(minStartFrame, startMax);
                if (minStartFrame < 0) minStartFrame = 0;
                startMax = maxEndFrame2 - desiredLen;
            }

            if (startMax < minStartFrame)
            {
                // 長さを短くして調整
                desiredLen = maxEndFrame2 - minStartFrame;
                startMax = minStartFrame;
            }

            if (desiredLen <= 0)
                break;

            int startFrame = (startMax > minStartFrame) ? NextRandom(minStartFrame, startMax) : minStartFrame;

            segments.Add(new PartSegment
            {
                Song = song,
                StartFrame = startFrame,
                FrameLength = desiredLen,
                IsFirst = false,
                IsLast = false,
            });

            // イの追加は fadeSamples 分だけ実効長が減る
            totalFramesSoFar += (desiredLen - fadeSamples);
        }

        // --- 末尾パート (ウ) の生成 ---
        {
            int remainingFrames = targetFrames - totalFramesSoFar;
            if (remainingFrames < 0)
            {
                // すでにオーバーしている場合は直近のパートを少し短くして調整
                int overshoot = -remainingFrames;
                var last = segments[segments.Count - 1];
                if (last.FrameLength > overshoot)
                {
                    last.FrameLength -= overshoot;
                    segments[segments.Count - 1] = last;
                    totalFramesSoFar -= overshoot;
                }
                remainingFrames = 0;
            }

            // 理論上必要な末尾パート長:
            // totalFramesSoFar + (finalLen - fade) = targetFrames
            int desiredFinalFrames = remainingFrames + fadeSamples;

            // 末尾パートが短くなり過ぎる場合、直前の中間パートを縮めてウを長くする (仕様 ②)
            if (desiredFinalFrames < minPartLenSamples && segments.Count >= 2)
            {
                int newFinalFrames = minPartLenSamples;
                int delta = newFinalFrames - desiredFinalFrames;

                var lastMid = segments[segments.Count - 1];
                // あまり極端に短くならないよう最低限の長さを残す
                if (!lastMid.IsFirst && lastMid.FrameLength - delta > fadeSamples * 2)
                {
                    lastMid.FrameLength -= delta;
                    segments[segments.Count - 1] = lastMid;
                    totalFramesSoFar -= delta;
                    desiredFinalFrames = newFinalFrames;
                }
            }

            if (desiredFinalFrames < fadeSamples + 1)
                desiredFinalFrames = fadeSamples + 1;

            bool finalCreated = false;

            // 「最後にキュー取得された元楽曲」を末尾に使う必要があるので、
            // Dequeue を繰り返しながら末尾パート候補を探す。
            for (int tryCount = 0; tryCount < sources.Count * 2 && !finalCreated; tryCount++)
            {
                var song = queue.Dequeue();
                int totalFrames = song.FrameCount;

                int maxEndFrame = (int)(totalFrames * 0.8); // 終了位置は 80% 以前
                if (maxEndFrame <= 0) maxEndFrame = totalFrames;

                int maxLenAllowed = Math.Min(maxEndFrame, maxPartLenSamples);
                int minLenAllowed = Math.Min(minPartLenSamples, maxLenAllowed);

                if (maxLenAllowed <= 0)
                {
                    // この曲は末尾パートとして使えない (極端に短いなど)
                    continue;
                }

                if (desiredFinalFrames > maxLenAllowed)
                {
                    // この楽曲では希望する長さの末尾パートを構成できない。
                    // 仕様どおり、これは「末尾」とせず、もう 1 つの中間パートとして挟み込む。

                    int lenForMid = maxLenAllowed;
                    if (lenForMid <= fadeSamples + 1)
                        continue;

                    int baseRandLen = NextRandom(
                        Math.Max(1, minLenAllowed),
                        Math.Max(1, lenForMid));

                    int minStartFrame = (int)(totalFrames * 0.2);
                    int startFrameMax = maxEndFrame - baseRandLen;
                    if (startFrameMax < minStartFrame) startFrameMax = minStartFrame;

                    int startFrame = (startFrameMax > minStartFrame) ? NextRandom(minStartFrame, startFrameMax) : minStartFrame;

                    segments.Add(new PartSegment
                    {
                        Song = song,
                        StartFrame = startFrame,
                        FrameLength = baseRandLen,
                        IsFirst = false,
                        IsLast = false,
                    });

                    totalFramesSoFar += (baseRandLen - fadeSamples);
                    remainingFrames = targetFrames - totalFramesSoFar;
                    desiredFinalFrames = remainingFrames + fadeSamples;
                    continue;
                }

                // 末尾パート用の理想基準長 (基準パート長 * 70%～100% の乱数)
                int baseRand = NextRandom(
                    Math.Max(desiredFinalFrames, minLenAllowed),
                    Math.Max(desiredFinalFrames, maxLenAllowed));

                // 「その楽曲の長さ - 基準パート長*乱数」以降かつ 20% 以降で開始する
                int startLimitFromTail = totalFrames - baseRand;
                int startLimit20 = (int)(totalFrames * 0.2);
                int minStart = Math.Max(startLimitFromTail, startLimit20);

                // target 長をぴったりにするための開始位置
                int startFrameForDesired = totalFrames - desiredFinalFrames;

                // 開始位置が制約を満たさない場合は、この楽曲では末尾パートを構成できない
                if (startFrameForDesired < minStart)
                    continue;

                finalCreated = true;

                segments.Add(new PartSegment
                {
                    Song = song,
                    StartFrame = startFrameForDesired,
                    FrameLength = desiredFinalFrames,
                    IsFirst = false,
                    IsLast = true,
                });

                totalFramesSoFar += (desiredFinalFrames - fadeSamples);
            }

            if (!finalCreated)
            {
                // どうしても仕様どおりの末尾パートを作れなかった場合:
                // 直近のパートを末尾パート扱いにして処理を完遂 (仕様違反だが例外で落とさないようにする)
                var last = segments[segments.Count - 1];
                last.IsLast = true;
                segments[segments.Count - 1] = last;
            }
        }

        // ********** 最終的なフレーム数を再計算・調整 **********

        int sumFrames = segments.Sum(s => s.FrameLength);
        int numParts = segments.Count;
        int effectiveFrames = sumFrames - Math.Max(0, numParts - 1) * fadeSamples; // crossfade の重なり分を差し引く

        if (effectiveFrames <= 0)
            throw new InvalidOperationException("パート構成の計算に失敗しました。");

        // targetFrames と実際の長さが微妙にズレている場合、末尾パートを微調整
        if (effectiveFrames > targetFrames)
        {
            int diff = effectiveFrames - targetFrames;
            var last = segments[segments.Count - 1];
            if (last.FrameLength > diff)
            {
                last.FrameLength -= diff;
                segments[segments.Count - 1] = last;
                effectiveFrames -= diff;
            }
        }
        else if (effectiveFrames < targetFrames)
        {
            int diff = targetFrames - effectiveFrames;
            var last = segments[segments.Count - 1];

            // 80% 制約を超えない範囲で末尾パートを少しだけ伸ばす
            int maxExtra = last.Song.FrameCount - last.StartFrame - last.FrameLength; // 完全な末尾までの余裕
            int maxEndLimit = (int)(last.Song.FrameCount * 0.8) - (last.StartFrame + last.FrameLength);
            if (maxEndLimit < 0) maxEndLimit = 0;
            maxExtra = Math.Min(maxExtra, maxEndLimit);

            int extra = Math.Min(diff, maxExtra);
            if (extra > 0)
            {
                last.FrameLength += extra;
                segments[segments.Count - 1] = last;
                effectiveFrames += extra;
            }
        }

        // 最終的に出力するフレーム数 (targetFrames を優先)
        int outputFrames = Math.Min(targetFrames, effectiveFrames);
        int outputSamples = outputFrames * Channels;
        var outSamples = new short[outputSamples];

        // ********** 実際のサンプルミックス処理 (crossfade を含む) **********

        int outFramePos = 0;

        for (int i = 0; i < segments.Count; i++)
        {
            var seg = segments[i];
            int startFrame = seg.StartFrame;
            int lenFrames = seg.FrameLength;
            var songSamples = seg.Song.Samples;

            if (i == 0)
            {
                // 先頭パート: 普通にコピー (後続パートの crossfade で末尾の一部は上書きされる)
                int framesToCopy = Math.Min(lenFrames, outputFrames - outFramePos);
                for (int f = 0; f < framesToCopy; f++)
                {
                    int srcIndex = (startFrame + f) * Channels;
                    int dstIndex = (outFramePos + f) * Channels;
                    if (dstIndex + 1 >= outSamples.Length) break;

                    outSamples[dstIndex] = songSamples[srcIndex];
                    outSamples[dstIndex + 1] = songSamples[srcIndex + 1];
                }

                outFramePos += framesToCopy;
            }
            else
            {
                // 2 パート目以降: 前のパートの末尾と mixFadeMsecs 分だけ重ね合わせる
                int overlapStart = outFramePos - fadeSamples; // クロスフェード部分の開始位置 (フレーム)

                // フェード区間のミックス
                for (int f = 0; f < fadeSamples && f < lenFrames; f++)
                {
                    int frameIndex = overlapStart + f;
                    if (frameIndex < 0 || frameIndex >= outputFrames) continue;

                    int dstIndex = frameIndex * Channels;
                    if (dstIndex + 1 >= outSamples.Length) break;

                    int srcIndexNew = (startFrame + f) * Channels;

                    short oldL = outSamples[dstIndex];
                    short oldR = outSamples[dstIndex + 1];

                    short newL = songSamples[srcIndexNew];
                    short newR = songSamples[srcIndexNew + 1];

                    // 線形フェード: 前が 1→0, 後ろが 0→1
                    double t = (fadeSamples <= 1) ? 1.0 : (double)f / (fadeSamples - 1);
                    double fadeIn = t;
                    double fadeOut = 1.0 - t;

                    int mixedL = (int)(oldL * fadeOut + newL * fadeIn);
                    int mixedR = (int)(oldR * fadeOut + newR * fadeIn);

                    outSamples[dstIndex] = ClampToInt16(mixedL);
                    outSamples[dstIndex + 1] = ClampToInt16(mixedR);
                }

                // フェード部分より後ろの区間は、新しいパートをそのまま貼り付ける
                int nonOverlapFrames = lenFrames - fadeSamples;
                if (nonOverlapFrames > 0)
                {
                    for (int f = fadeSamples; f < lenFrames; f++)
                    {
                        int frameIndex = outFramePos + (f - fadeSamples);
                        if (frameIndex >= outputFrames) break;

                        int dstIndex = frameIndex * Channels;
                        if (dstIndex + 1 >= outSamples.Length) break;

                        int srcIndex = (startFrame + f) * Channels;
                        outSamples[dstIndex] = songSamples[srcIndex];
                        outSamples[dstIndex + 1] = songSamples[srcIndex + 1];
                    }

                    outFramePos += Math.Max(0, nonOverlapFrames);
                    if (outFramePos > outputFrames)
                        outFramePos = outputFrames;
                }
            }
        }

        // ********** 出力 WAV (ヘッダつきバイト列) の生成 **********

        using var outMs = new MemoryStream();
        var outWaveFormat = new WaveFormat(SampleRate, BitsPerSample, Channels);

        // WaveFileWriter にヘッダと PCM データを書かせる
        using (var writer = new WaveFileWriter(outMs, outWaveFormat))
        {
            var buffer = new byte[outSamples.Length * 2];
            Buffer.BlockCopy(outSamples, 0, buffer, 0, buffer.Length);
            writer.Write(buffer, 0, buffer.Length);
        }

        // メモリ上に作成された WAV バイト列を返す
        return outMs.ToArray();
    }

    // short の範囲にクリップするヘルパー
    static short ClampToInt16(int v)
    {
        if (v > short.MaxValue) return short.MaxValue;
        if (v < short.MinValue) return short.MinValue;
        return (short)v;
    }
}

















// ChatGPT 5 Pro で生成
/// <summary>
/// ルート名前空間上に置くユーティリティクラス。
/// WAV(PCM 16bit / 44.1kHz / Stereo) をメモリ上だけでミックスして別の WAV を生成する。
/// </summary>
public static class AiMixTwoMusicsLib
{
    /// <summary>
    /// 2つの WAV をメモリ上でミックスし、targetLengthMsecs ミリ秒の長さを持つ新しい WAV を生成する。
    /// </summary>
    /// <param name="sourceWav1">1つ目の WAV バイナリデータ</param>
    /// <param name="sourceWav2">2つ目の WAV バイナリデータ</param>
    /// <param name="targetLengthMsecs">出力 WAV の長さ (ミリ秒)</param>
    /// <param name="mixFadeMsecs">各セグメントをクロスフェードする時間 (ミリ秒)</param>
    /// <returns>生成された WAV バイト配列</returns>
    public static byte[] MixTwoMusicsSimple(
        ReadOnlyMemory<byte> sourceWav1,
        ReadOnlyMemory<byte> sourceWav2,
        int targetLengthMsecs, int mixFadeMsecs)
    {
        // 入力値の妥当性チェック
        if (targetLengthMsecs <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(targetLengthMsecs), "targetLengthMsecs は正の値である必要があります。");
        }
        if (mixFadeMsecs < 0)
        {
            throw new ArgumentOutOfRangeException(nameof(mixFadeMsecs), "mixFadeMsecs は 0 以上である必要があります。");
        }
        if (sourceWav1.IsEmpty || sourceWav2.IsEmpty)
        {
            throw new ArgumentException("sourceWav1 / sourceWav2 は空であってはなりません。");
        }

        // 乱数生成器 (毎回異なるシードを利用する)
        var rng = CreateRandom();

        // WAV をパースして PCM 16bit / 44.1kHz / Stereo かどうかチェックしつつ、短整数配列として取得
        var wav1 = ParsePcm16Wave(sourceWav1);
        var wav2 = ParsePcm16Wave(sourceWav2);

        // 2つの WAV のフォーマットが完全に一致しているかチェック
        if (wav1.SampleRate != wav2.SampleRate ||
            wav1.Channels != wav2.Channels ||
            wav1.BitsPerSample != wav2.BitsPerSample)
        {
            throw new InvalidDataException("2つの WAV のフォーマットが一致していません。");
        }

        var sampleRate = wav1.SampleRate;   // 44100 固定の想定
        var channels = wav1.Channels;     // 2 固定の想定

        // targetLengthMsecs をフレーム数(ステレオフレーム単位)に変換
        // 1フレーム = 1サンプル(全チャンネル分) = 1 / sampleRate 秒
        int targetFrames = checked((int)Math.Round((double)targetLengthMsecs * (double)sampleRate / 1000.0));
        if (targetFrames <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(targetLengthMsecs), "targetLengthMsecs が短すぎて 1 フレームも生成できません。");
        }

        // ここから、sourceStart / sourceMiddle / sourceEnd を定義する
        // 各位置はすべてフレーム単位で扱う (サンプルではなく、ステレオフレーム)

        int totalFrames1 = wav1.TotalFrames;
        int totalFrames2 = wav2.TotalFrames;

        if (totalFrames1 <= 0 || totalFrames2 <= 0)
        {
            throw new InvalidDataException("入力 WAV のオーディオデータが空です。");
        }

        // sourceStart: wav1 の 0% ～ 80%
        int startEndFrame1 = (int)(totalFrames1 * 0.8);
        // sourceMiddle: wav1 の 20% ～ 80%
        int middleStartFrame1 = (int)(totalFrames1 * 0.2);
        int middleEndFrame1 = startEndFrame1;
        int middleLenFrames1 = Math.Max(0, middleEndFrame1 - middleStartFrame1);

        // sourceEnd: wav2 の 20% ～ 100%
        int endStartFrame2 = (int)(totalFrames2 * 0.2);
        int endLenFrames2 = totalFrames2 - endStartFrame2;

        // あまりに短いとア/イ/ウ が定義できないのでエラーとする
        if (startEndFrame1 <= 0 || middleLenFrames1 <= 0 || endLenFrames2 <= 0)
        {
            throw new InvalidDataException("入力 WAV が短すぎるため、start/middle/end を定義できません。");
        }

        int startMaxFrames = startEndFrame1;   // アの最大長
        int middleMaxFrames = middleLenFrames1; // イ1つあたりの最大長
        int endMaxFrames = endLenFrames2;    // ウの最大長

        // ア(1個) + イ(N個) + ウ(1個) の総数
        // N はできるだけ少なくしたいので、まず必要最低限の N を計算する。
        // ここでは「各セグメントの最大長をフルに使ったとき」の上限から決める。
        // targetFrames <= startMax + N * middleMax + endMax を満たす最小の N を求める。
        int minimalN = 0;
        long maxWithoutMiddle = (long)startMaxFrames + endMaxFrames;

        if (targetFrames > maxWithoutMiddle)
        {
            if (middleMaxFrames <= 0)
            {
                throw new InvalidOperationException("中間部分が存在しないため、ターゲットの長さを満たせません。");
            }

            // ceil((target - start - end) / middleMax)
            minimalN = (int)Math.Ceiling(
                (targetFrames - maxWithoutMiddle) / (double)middleMaxFrames
            );
            if (minimalN < 0) minimalN = 0;
        }

        // セグメント数
        int N = minimalN;
        int segmentCount = N + 2; // ア + N個のイ + ウ

        // あまりに短い targetLength の場合、セグメント数よりもフレーム数が少ないと構成不可能。
        // (実際にはそんな短い音楽はあり得ないと想定)
        if (targetFrames < segmentCount)
        {
            throw new ArgumentOutOfRangeException(nameof(targetLengthMsecs),
                "targetLengthMsecs が短すぎて、ア・イ・ウ を最低 1 フレームずつ配置できません。");
        }

        // 各セグメントの最大長 (フレーム単位) 配列を作る
        var maxFramesPerSegment = new int[segmentCount];
        maxFramesPerSegment[0] = startMaxFrames; // ア
        for (int i = 1; i <= N; i++)
        {
            maxFramesPerSegment[i] = middleMaxFrames; // 各イ
        }
        maxFramesPerSegment[segmentCount - 1] = endMaxFrames; // ウ

        // セグメントを最大長まで使った場合の合計
        long sumMaxFrames = maxFramesPerSegment.Aggregate<int, long>(0, (acc, v) => acc + v);

        // クロスフェード無しの状態でも target を満たせない場合はそもそも不可能
        if (targetFrames > sumMaxFrames)
        {
            throw new InvalidOperationException("与えられた入力長では targetLengthMsecs を満たせません。");
        }

        // ここから「クロスフェード込み」で targetLengthMsecs にぴったり合う長さを決める。
        // 合計フレーム数(セグメント長の総和)を S、フェードフレーム数を F とすると:
        // 出力合計フレーム = S - F * (segmentCount - 1) = targetFrames
        // よって S = targetFrames + F * (segmentCount - 1)
        //
        // ただし、F (フレーム) は以下を満たす必要がある:
        // 1) ユーザー指定の mixFadeMsecs 相当以下
        // 2) セグメントの最小長の 1/10 以下
        // 3) セグメント最大長の合計を超えない (容量制限)
        //
        // ここでは、F を「容量」と「ユーザー指定」を満たす上限から出発し、
        // セグメント最小長の制約を満たすまで繰り返し調整する。

        // 1) ユーザー指定 (ミリ秒→フレーム変換)
        int requestedFadeFrames = (int)Math.Round(mixFadeMsecs * sampleRate / 1000.0);
        if (requestedFadeFrames < 0) requestedFadeFrames = 0;

        // 2) 容量による上限:
        //   S <= sumMaxFrames なので target + F*(segmentCount-1) <= sumMaxFrames
        //   -> F <= (sumMaxFrames - target) / (segmentCount - 1)
        int maxFadeByCapacity = 0;
        if (segmentCount > 1 && sumMaxFrames > targetFrames)
        {
            maxFadeByCapacity = (int)((sumMaxFrames - targetFrames) / (segmentCount - 1));
            if (maxFadeByCapacity < 0) maxFadeByCapacity = 0;
        }

        int fadeFramesCandidate = Math.Min(requestedFadeFrames, maxFadeByCapacity);
        if (fadeFramesCandidate < 0) fadeFramesCandidate = 0;

        // セグメント長と F を確定させるためのループ (多くても 2～3 回程度で収束する想定)
        int[] segmentFrames = null!;
        int fadeFramesFinal = 0;
        const int MaxFadeIteration = 4;

        for (int iter = 0; iter < MaxFadeIteration; iter++)
        {
            // この時点での F 候補を使ったセグメント総和 S を計算対象とする
            int targetSumFrames = targetFrames + fadeFramesCandidate * (segmentCount - 1);

            if (targetSumFrames < segmentCount)
            {
                // ここまで来るケースはほぼ無いが、念のため保険
                throw new InvalidOperationException("フェード長の制約により、セグメント構成ができません。");
            }

            if (targetSumFrames > sumMaxFrames)
            {
                // 容量チェック (理論上ここには来ないはずだが保険)
                // 容量を超えてしまう場合は F を減らして再トライ
                if (fadeFramesCandidate == 0)
                {
                    throw new InvalidOperationException("フェード長を 0 にしても targetLength に到達できません。");
                }
                fadeFramesCandidate = Math.Max(0, fadeFramesCandidate / 2);
                continue;
            }

            // 現在の F 候補に対して、各セグメントの長さ (フレーム数) を割り当てる。
            if (!TryAllocateSegmentFrames(maxFramesPerSegment, targetSumFrames, rng, out segmentFrames))
            {
                // 正常ならここに来ないが、万一失敗したら F を少し縮めてリトライする。
                if (fadeFramesCandidate == 0)
                {
                    throw new InvalidOperationException("セグメント長の割り当てに失敗しました。");
                }
                fadeFramesCandidate = Math.Max(0, fadeFramesCandidate / 2);
                continue;
            }

            // フェアネス条件 (最短 >= 最長の 80%) を満たすように、
            // 可能な範囲で長さを少しならし処理する。
            SmoothDurationsForFairness(segmentFrames, maxFramesPerSegment);

            // この割り当てでの最小セグメント長を取得
            int minSegmentFrames = segmentFrames.Min();

            // mixFadeMsecs の制約その2:
            // F <= (最短セグメント長) / 10
            int maxFadeByMinSegment = minSegmentFrames / 10;
            int fadeFramesByMin = Math.Max(0, maxFadeByMinSegment);

            int newFade = Math.Min(fadeFramesCandidate, fadeFramesByMin);

            if (newFade == fadeFramesCandidate)
            {
                // 現在の fadeFramesCandidate はすべての制約を満たしているので確定
                fadeFramesFinal = fadeFramesCandidate;
                break;
            }

            // まだ最短セグメント長に対して長すぎるので、F を小さくして再トライ
            fadeFramesCandidate = newFade;
        }

        // フェードがループ中に決まらなかった場合は、最後の候補値を採用し直す
        if (segmentFrames == null)
        {
            // ありえないはずだが保険としてもう一度、フェード 0 で割り当てを試みる
            fadeFramesFinal = 0;
            int targetSumFrames = targetFrames; // F=0 なので S=target
            if (!TryAllocateSegmentFrames(maxFramesPerSegment, targetSumFrames, rng, out segmentFrames))
            {
                throw new InvalidOperationException("セグメント長の割り当てに失敗しました。");
            }
            SmoothDurationsForFairness(segmentFrames, maxFramesPerSegment);
        }

        // ここで、
        //   segmentFrames の合計 = targetFrames + fadeFramesFinal * (segmentCount - 1)
        // となっているはずなので、
        // 出力合計フレーム = 上記合計 - fadeFramesFinal * (segmentCount -1) = targetFrames
        // となる。

        // セグメントが WAV 内のどの位置を使うかを決定する
        // ア: wav1 の先頭から指定フレーム数ぶん
        // イ: wav1 の middle (20%～80%) の中からランダムな開始位置
        // ウ: wav2 の末尾側から指定フレーム数ぶん (常に曲の最後で終わる)
        var segments = new SegmentSpec[segmentCount];

        // ア
        segments[0] = new SegmentSpec
        {
            Source = wav1,
            StartFrame = 0,
            LengthFrames = segmentFrames[0]
        };

        // イ (N 個)
        for (int i = 0; i < N; i++)
        {
            int segIndex = 1 + i;
            int len = segmentFrames[segIndex];

            // 中間部分 (middleStartFrame1 ～ middleEndFrame1) の範囲内で、
            // 長さ len の区間をランダムに切り出す。
            int availableLen = middleLenFrames1;
            if (len > availableLen)
            {
                // 理論上ここには来ないはずだが保険
                len = availableLen;
            }

            int maxOffset = availableLen - len;
            int offset = 0;
            if (maxOffset > 0)
            {
                offset = rng.Next(0, maxOffset + 1);
            }

            int startFrame = middleStartFrame1 + offset;

            segments[segIndex] = new SegmentSpec
            {
                Source = wav1,
                StartFrame = startFrame,
                LengthFrames = len
            };
        }

        // ウ (最後のセグメント)
        {
            int lastIndex = segmentCount - 1;
            int len = segmentFrames[lastIndex];
            if (len > endLenFrames2)
            {
                // 保険
                len = endLenFrames2;
            }

            // 「末尾部分」が必ず末尾となるように、曲の最後に合わせて切り出す
            int startFrame = totalFrames2 - len;
            if (startFrame < endStartFrame2)
            {
                // 中間の定義より手前に出てしまわないようにクリップ
                startFrame = endStartFrame2;
                if (startFrame + len > totalFrames2)
                {
                    startFrame = totalFrames2 - len;
                }
            }

            segments[lastIndex] = new SegmentSpec
            {
                Source = wav2,
                StartFrame = startFrame,
                LengthFrames = len
            };
        }

        // ここまでで、ア・イ・ウ各セグメントの切り出し位置と長さが決まった。
        // 次に、これらを実際にクロスフェードして 1 本の PCM データにミックスする。

        int outputFrames = targetFrames; // 出力フレーム数
        short[] mixedSamples = RenderSegmentsToSamples(
            segments,
            fadeFramesFinal,
            channels,
            outputFrames);

        // short[] → byte[] へ変換 (Little Endian)
        byte[] mixedPcmBytes = new byte[mixedSamples.Length * sizeof(short)];
        MemoryMarshal.Cast<short, byte>(mixedSamples.AsSpan()).CopyTo(mixedPcmBytes.AsSpan());

        // 最後に、NAudio の WaveFileWriter を用いてヘッダ付きの WAV として書き出す。
        using var outStream = new MemoryStream();
        using (var writer = new WaveFileWriter(outStream, wav1.WaveFormat))
        {
            // WAV のヘッダは WaveFileWriter が自動生成する
            writer.Write(mixedPcmBytes, 0, mixedPcmBytes.Length);
            writer.Flush();
        }

        // MemoryStream から最終的な WAV バイト配列を取得して返す
        return outStream.ToArray();
    }

    #region 内部ヘルパー

    /// <summary>
    /// WAV ファイル(バイト列)をパースして、PCM 16bit / 44.1kHz / Stereo のみを受け付ける。
    /// </summary>
    private static WavPcm16Data ParsePcm16Wave(ReadOnlyMemory<byte> wavBytes)
    {
        // ReadOnlyMemory から配列を取得 (必ずしも ArraySegment とは限らないので ToArray でコピー)
        byte[] data = wavBytes.ToArray();

        try
        {
            using var ms = new MemoryStream(data, writable: false);
            using var reader = new WaveFileReader(ms);

            var format = reader.WaveFormat;

            // フォーマットチェック
            if (format.Encoding != WaveFormatEncoding.Pcm)
            {
                throw new InvalidDataException("WAV フォーマットは PCM である必要があります。");
            }
            if (format.SampleRate != 44100)
            {
                throw new InvalidDataException("サンプリングレートは 44100 Hz 固定です。");
            }
            if (format.BitsPerSample != 16)
            {
                throw new InvalidDataException("サンプルあたりビット長は 16bit 固定です。");
            }
            if (format.Channels != 2)
            {
                throw new InvalidDataException("チャンネル数はステレオ(2ch)固定です。");
            }

            long byteLength = reader.Length; // WaveStream.Length はデータ部の長さ(バイト数)になる
            if (byteLength <= 0)
            {
                throw new InvalidDataException("WAV データ部の長さが 0 です。");
            }
            if (byteLength > int.MaxValue)
            {
                throw new InvalidDataException("データサイズが大きすぎます。");
            }

            int dataLen = (int)byteLength;

            // blockAlign (1フレームあたりのバイト数) で割り切れることを確認
            if (dataLen % format.BlockAlign != 0)
            {
                throw new InvalidDataException("データ長が BlockAlign で割り切れません。WAV が壊れている可能性があります。");
            }

            byte[] buffer = new byte[dataLen];

            int offset = 0;
            while (offset < dataLen)
            {
                int read = reader.Read(buffer, offset, dataLen - offset);
                if (read <= 0)
                {
                    break;
                }
                offset += read;
            }

            if (offset != dataLen)
            {
                throw new InvalidDataException("WAV データを最後まで読み取れませんでした。");
            }

            // 16bit PCM なので 2 バイトで 1 サンプル
            if (dataLen % 2 != 0)
            {
                throw new InvalidDataException("16bit PCM なのにデータ長が 2 の倍数ではありません。");
            }

            int sampleCount = dataLen / 2;
            short[] samples = new short[sampleCount];
            MemoryMarshal.Cast<byte, short>(buffer.AsSpan()).CopyTo(samples.AsSpan());

            return new WavPcm16Data(format, samples);
        }
        catch (EndOfStreamException ex)
        {
            // WAV が途中で切れているなどのパースエラー
            throw new InvalidDataException("WAV のパース中にエラーが発生しました。", ex);
        }
    }

    /// <summary>
    /// セグメント毎の最大長 maxFramesPerSegment[] を満たしつつ、
    /// 全体の合計フレーム長 targetSumFrames を満たすような各セグメント長を割り当てる。
    /// * ここではフェアネス条件は気にせず、単に 1 &lt;= length &lt;= maxLength を満たすような組合せを作る。
    /// </summary>
    private static bool TryAllocateSegmentFrames(
        int[] maxFramesPerSegment,
        int targetSumFrames,
        Random rng,
        out int[] segmentFrames)
    {
        int count = maxFramesPerSegment.Length;
        segmentFrames = new int[count];

        // セグメントそれぞれの最大長合計
        long sumMax = maxFramesPerSegment.Aggregate<int, long>(0, (acc, v) => acc + v);

        // 最小長は 1 フレームとする (0 長セグメントは扱いが難しいため禁止)
        int minPerSegment = 1;
        long minSum = (long)minPerSegment * count;

        if (targetSumFrames < minSum || targetSumFrames > sumMax)
        {
            // 理論的に解が存在しない
            return false;
        }

        // suffixMax[i] = i 以降の最大長合計
        var suffixMax = new long[count + 1];
        suffixMax[count] = 0;
        for (int i = count - 1; i >= 0; i--)
        {
            suffixMax[i] = suffixMax[i + 1] + maxFramesPerSegment[i];
        }

        int remaining = targetSumFrames;

        for (int i = 0; i < count; i++)
        {
            int segmentsLeft = count - i;

            // 残りのセグメント (自分以外) が最低 1 フレームずつ持つとして必要な最小値
            int minRemainingForOthers = (segmentsLeft - 1) * minPerSegment;

            // このセグメントが取り得る基本の範囲 [minForThis, maxForThis]
            int minForThis = minPerSegment;
            int maxForThis = maxFramesPerSegment[i];

            // 「残りのセグメントに最大限割り当てても targetSumFrames を超えない」ための下限
            // remaining - x <= suffixMax[i+1] → x >= remaining - suffixMax[i+1]
            int minFromMaxConstraint = (int)Math.Max(minPerSegment, remaining - (int)suffixMax[i + 1]);
            if (minFromMaxConstraint > minForThis)
            {
                minForThis = minFromMaxConstraint;
            }

            // 「残りのセグメントに最低 1 フレームずつ割り当てる」ための上限
            // remaining - x >= minRemainingForOthers → x <= remaining - minRemainingForOthers
            int maxFromMinConstraint = remaining - minRemainingForOthers;
            if (maxFromMinConstraint < maxForThis)
            {
                maxForThis = maxFromMinConstraint;
            }

            if (minForThis > maxForThis)
            {
                // 範囲が破綻した場合は失敗
                return false;
            }

            // ここから実際の値を決める。
            // なるべく「平均値 ±少しの乱数」に寄せることで、あまり極端な長さにならないようにする。
            double ideal = (double)remaining / segmentsLeft;
            double factor = 0.9 + rng.NextDouble() * 0.2; // 0.9～1.1 のランダム係数

            int candidate = (int)Math.Round(ideal * factor);
            if (candidate < minForThis) candidate = minForThis;
            if (candidate > maxForThis) candidate = maxForThis;

            segmentFrames[i] = candidate;
            remaining -= candidate;
        }

        // ここまでで remaining は 0 のはず
        return remaining == 0;
    }

    /// <summary>
    /// セグメント長が極端にばらつかないように、最短セグメントと最長セグメント間で
    /// 1フレームずつ長さを融通し合う簡易な平滑化処理。
    /// 「最短 &gt;= 最長の 80%」を目指すが、絶対保証ではない (無理なケースもある)。
    /// </summary>
    private static void SmoothDurationsForFairness(int[] segmentFrames, int[] maxFramesPerSegment)
    {
        const int MaxIterations = 64;
        int count = segmentFrames.Length;

        for (int iter = 0; iter < MaxIterations; iter++)
        {
            int min = segmentFrames.Min();
            int max = segmentFrames.Max();

            // フェアネス条件: min >= 0.8 * max
            if (min * 5 >= max * 4) // 5*min >= 4*max  <=> min >= 0.8*max
            {
                break; // 既に十分均衡している
            }

            // 最小値/最大値となっているインデックスを探す
            List<int> minIndices = new();
            List<int> maxIndices = new();

            for (int i = 0; i < count; i++)
            {
                if (segmentFrames[i] == min) minIndices.Add(i);
                if (segmentFrames[i] == max) maxIndices.Add(i);
            }

            if (minIndices.Count == 0 || maxIndices.Count == 0)
            {
                break;
            }

            bool changed = false;

            // 最小セグメントを +1、最大セグメントを -1 し、合計長を変えずに差を詰める
            foreach (int minIdx in minIndices)
            {
                if (segmentFrames[minIdx] >= maxFramesPerSegment[minIdx])
                {
                    // これ以上伸ばせない
                    continue;
                }

                foreach (int maxIdx in maxIndices)
                {
                    if (segmentFrames[maxIdx] <= 1)
                    {
                        // これ以上縮めると 1 フレームを割ってしまう
                        continue;
                    }

                    segmentFrames[minIdx]++;
                    segmentFrames[maxIdx]--;
                    changed = true;
                    break;
                }

                if (changed) break;
            }

            if (!changed)
            {
                // これ以上調整できない
                break;
            }
        }
    }

    /// <summary>
    /// セグメント情報。
    /// </summary>
    private class SegmentSpec
    {
        public WavPcm16Data Source { get; init; } = null!;
        public int StartFrame { get; init; }
        public int LengthFrames { get; init; }
    }

    /// <summary>
    /// セグメント列をクロスフェードして 1 本の PCM(short配列) にミックスする。
    /// </summary>
    private static short[] RenderSegmentsToSamples(
        SegmentSpec[] segments,
        int fadeFrames,
        int channels,
        int totalOutputFrames)
    {
        if (channels != 2)
        {
            // ここではステレオ固定の実装としておく
            throw new NotSupportedException("RenderSegmentsToSamples は 2ch ステレオのみ対応です。");
        }

        int totalSamples = checked(totalOutputFrames * channels);
        short[] output = new short[totalSamples];

        int outPosFrames = 0; // 非オーバーラップ部分の末尾フレーム位置
        bool isFirstSegment = true;

        foreach (var seg in segments)
        {
            if (seg.LengthFrames <= 0)
            {
                continue;
            }

            var src = seg.Source;
            if (src.Channels != channels)
            {
                throw new InvalidDataException("セグメントのチャンネル数が不一致です。");
            }

            int srcTotalFrames = src.TotalFrames;
            int startFrame = seg.StartFrame;
            int lenFrames = seg.LengthFrames;

            // セグメントが元データの範囲外を参照していないかチェック
            if (startFrame < 0 || startFrame + lenFrames > srcTotalFrames)
            {
                throw new InvalidDataException("セグメントがソース WAV の範囲外を参照しています。");
            }

            int outputStartFrame;
            if (isFirstSegment || fadeFrames <= 0)
            {
                // 最初のセグメントまたはフェード無しの場合は普通に連結
                outputStartFrame = outPosFrames;
            }
            else
            {
                // 2 個目以降のセグメントでは、fadeFrames ぶん手前から書き込み開始してクロスフェードする
                outputStartFrame = outPosFrames - fadeFrames;
                if (outputStartFrame < 0) outputStartFrame = 0;
            }

            var srcSamples = src.Samples;

            for (int k = 0; k < lenFrames; k++)
            {
                int destFrame = outputStartFrame + k;
                if (destFrame < 0 || destFrame >= totalOutputFrames)
                {
                    // 安全のため範囲外書き込みは無視
                    break;
                }

                int srcFrame = startFrame + k;
                int srcIndex = srcFrame * channels;

                short srcL = srcSamples[srcIndex];
                short srcR = srcSamples[srcIndex + 1];

                int destIndex = destFrame * channels;

                if (!isFirstSegment && fadeFrames > 0 && k < fadeFrames)
                {
                    // フェード部分: 既に書かれている値(output)と線形にブレンドする。
                    // t=0 → 旧セグメント100%、t=1 → 新セグメント100% に近い形でミックスする。
                    double t = (double)k / fadeFrames; // 0 ～ 1未満
                    double wNew = t;
                    double wOld = 1.0 - t;

                    short oldL = output[destIndex];
                    short oldR = output[destIndex + 1];

                    int mixedL = (int)(oldL * wOld + srcL * wNew);
                    int mixedR = (int)(oldR * wOld + srcR * wNew);

                    // クリップ処理 (16bit の範囲に収める)
                    if (mixedL > short.MaxValue) mixedL = short.MaxValue;
                    else if (mixedL < short.MinValue) mixedL = short.MinValue;
                    if (mixedR > short.MaxValue) mixedR = short.MaxValue;
                    else if (mixedR < short.MinValue) mixedR = short.MinValue;

                    output[destIndex] = (short)mixedL;
                    output[destIndex + 1] = (short)mixedR;
                }
                else
                {
                    // フェードなし部分はそのまま上書き
                    output[destIndex] = srcL;
                    output[destIndex + 1] = srcR;
                }
            }

            if (isFirstSegment || fadeFrames <= 0)
            {
                outPosFrames += lenFrames;
            }
            else
            {
                outPosFrames = outputStartFrame + lenFrames;
            }

            isFirstSegment = false;
        }

        // outPosFrames は理論上 totalOutputFrames と一致しているはずだが、
        // 万が一不足している場合は、末尾を 0 で埋めておく (現状実装ではほぼ発生しない想定)。
        // ※足りない場合でも output 配列は既に確保済みなので、そのまま返して問題ない。

        return output;
    }

    /// <summary>
    /// 乱数生成器 (毎回異なるシードを利用するための小さなヘルパー)
    /// </summary>
    private static Random CreateRandom()
    {
        // Environment.TickCount と Guid ハッシュを XOR してシードを作る
        // ※ 簡易だが毎回そこそこ違うシードになる
        int seed = unchecked(Environment.TickCount ^ Guid.NewGuid().GetHashCode());
        return new Random(seed);
    }

    /// <summary>
    /// 16bit PCM WAV データをまとめて保持する内部クラス。
    /// </summary>
    private sealed class WavPcm16Data
    {
        public WaveFormat WaveFormat { get; }
        public short[] Samples { get; }

        public int Channels => WaveFormat.Channels;
        public int SampleRate => WaveFormat.SampleRate;
        public int BitsPerSample => WaveFormat.BitsPerSample;

        /// <summary>
        /// ステレオフレーム数 (1フレーム = 全チャンネル1サンプルぶん)
        /// </summary>
        public int TotalFrames => Samples.Length / Channels;

        public WavPcm16Data(WaveFormat format, short[] samples)
        {
            WaveFormat = format ?? throw new ArgumentNullException(nameof(format));
            Samples = samples ?? throw new ArgumentNullException(nameof(samples));

            if (samples.Length % format.Channels != 0)
            {
                throw new InvalidDataException("サンプル数がチャンネル数で割り切れません。");
            }
        }
    }

    #endregion
}



// ChatGPT 5 で生成
/// <summary>
/// ルート名前空間上のユーティリティクラス。
/// </summary>
public static class AiGenerateExactLengthMusicLib
{
    /// <summary>
    /// 指定された WAV バイナリ (PCM 16bit / 44.1kHz / stereo 固定) を読み込み、
    /// 指定ミリ秒の長さになるように、頭・中間・末尾を切り貼り＋クロスフェードして新しい WAV を生成する。
    /// </summary>
    /// <param name="sourceWav">入力元 WAV ファイルのバイナリデータ (ヘッダ込み)。</param>
    /// <param name="targetLengthMsecs">出力したい WAV の長さ [msec]。</param>
    /// <param name="mixFadeMsecs">セグメント切り替え時のクロスフェード長 [msec]。</param>
    /// <returns>新たに確保された target WAV のバイト配列。</returns>
    public static byte[] GenerateExactLengthMusic(
        ReadOnlyMemory<byte> sourceWav,
        int targetLengthMsecs,
        int mixFadeMsecs)
    {
        // 引数チェック
        if (targetLengthMsecs <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(targetLengthMsecs),
                "targetLengthMsecs は 1 以上でなければなりません。");
        }

        if (sourceWav.IsEmpty)
        {
            throw new ArgumentException("sourceWav が空です。", nameof(sourceWav));
        }

        if (mixFadeMsecs < 0)
        {
            throw new ArgumentOutOfRangeException(nameof(mixFadeMsecs),
                "mixFadeMsecs は 0 以上でなければなりません。");
        }

        // ReadOnlyMemory はこの関数の外側で管理されるので、内部処理用に必ずコピーしておく。
        // (仕様上「targetWav のメモリは本関数内で確保せよ」とあるため、戻り値は別配列で返す。)
        byte[] sourceBytes = sourceWav.ToArray();

        // NAudio を用いて WAV をパース
        using var ms = new MemoryStream(sourceBytes, writable: false);
        using var reader = new WaveFileReader(ms);

        var fmt = reader.WaveFormat;

        // フォーマットチェック (PCM, 44.1kHz, Stereo, 16bit)
        // ★注意★: 想定以外のフォーマットは全て例外とする。
        if (fmt.Encoding != WaveFormatEncoding.Pcm ||
            fmt.SampleRate != 44100 ||
            fmt.BitsPerSample != 16 ||
            fmt.Channels != 2)
        {
            throw new InvalidDataException(
                $"サポートされない WAV フォーマットです。" +
                $"Encoding={fmt.Encoding}, SampleRate={fmt.SampleRate}, BitsPerSample={fmt.BitsPerSample}, Channels={fmt.Channels}");
        }

        long dataLengthBytesLong = reader.Length; // PCM データ部のみの長さ (ヘッダ除く)
        if (dataLengthBytesLong <= 0)
        {
            throw new InvalidDataException("PCM データが空です。");
        }

        if (dataLengthBytesLong > int.MaxValue)
        {
            // ★注意★: 配列インデックスは int なので、2GB 超の WAV はここでは対象外とする。
            throw new NotSupportedException("入力 WAV が大きすぎてメモリ内で扱えません。");
        }

        int dataLengthBytes = (int)dataLengthBytesLong;
        int blockAlign = fmt.BlockAlign; // 1 フレームあたりのバイト数 (stereo 16bit なら 4)
        if (blockAlign <= 0 || dataLengthBytes % blockAlign != 0)
        {
            throw new InvalidDataException("WAV の blockAlign またはデータ長が不正です。");
        }

        int totalFrames = dataLengthBytes / blockAlign;
        if (totalFrames <= 0)
        {
            throw new InvalidDataException("PCM データが短すぎます。");
        }

        // PCM 16bit を float [-1,1] に変換
        byte[] pcmBytes = new byte[dataLengthBytes];
        int bytesRead = reader.Read(pcmBytes, 0, pcmBytes.Length);
        if (bytesRead != dataLengthBytes)
        {
            throw new InvalidDataException("PCM データを最後まで読み込めませんでした。");
        }

        int channels = fmt.Channels; // 2
        float[] sourceSamples = new float[totalFrames * channels];

        // ★注意★: 16bit little-endian を正しく decode すること。
        for (int frame = 0; frame < totalFrames; frame++)
        {
            int frameByteIndex = frame * blockAlign;
            for (int ch = 0; ch < channels; ch++)
            {
                int byteIndex = frameByteIndex + ch * 2;
                short sample16 = BinaryPrimitives.ReadInt16LittleEndian(pcmBytes.AsSpan(byteIndex, 2));
                sourceSamples[frame * channels + ch] = sample16 / 32768f; // -32768 ≒ -1.0f
            }
        }

        int targetFrames = MillisToFrames(targetLengthMsecs, fmt.SampleRate);
        if (targetFrames <= 0)
        {
            throw new InvalidOperationException("targetLengthMsecs が短すぎて、1 フレームも生成できません。");
        }

        // 長さがほぼ同じ (1 フレーム以内) の場合は、元の WAV をそのまま返す。
        // (ここでも必ず新しい配列を返すため、sourceBytes をそのまま返しても要件を満たす。)
        if (Math.Abs(targetFrames - totalFrames) <= 1)
        {
            return sourceBytes;
        }

        // 乱数シードは毎回異なるようにする (切れ目位置を毎回変えるため)
        Random rng = CreateRandom();

        List<SegmentPlan> segments;
        int fadeFrames;

        if (targetFrames > totalFrames)
        {
            // 伸長 (短い曲を長くする)
            if (!PlanSegmentsForLengthen(
                    totalFrames,
                    targetFrames,
                    mixFadeMsecs,
                    fmt.SampleRate,
                    rng,
                    out segments,
                    out fadeFrames))
            {
                throw new InvalidOperationException("音源の伸長プラン作成に失敗しました。");
            }
        }
        else
        {
            // 短縮 (長い曲を短くする)
            if (!PlanSegmentsForShorten(
                    totalFrames,
                    targetFrames,
                    mixFadeMsecs,
                    fmt.SampleRate,
                    rng,
                    out segments,
                    out fadeFrames))
            {
                throw new InvalidOperationException("音源の短縮プラン作成に失敗しました。");
            }
        }

        // セグメントプランに従ってミックス (クロスフェード) を実行
        // ★注意★: ここで最終的なフレーム数が targetFrames になるように配置する。
        float[] mixedSamples = RenderSegments(
            segments,
            sourceSamples,
            channels,
            fadeFrames,
            targetFrames);

        // float[-1,1] → 16bit PCM へ戻す
        byte[] pcmOutBytes = ConvertFloatToPcm16(mixedSamples);

        // WAV ヘッダを付与して完成させる
        byte[] resultWav = BuildPcm16Wave(pcmOutBytes, fmt.SampleRate, channels);

        return resultWav;
    }

    #region 内部ヘルパー

    /// <summary>
    /// セグメント情報 (元 WAV のどこから何フレーム切り出すか)。
    /// </summary>
    private sealed class SegmentPlan
    {
        public int SourceStartFrame; // 元音源上の開始フレーム位置
        public int LengthFrames;     // このセグメントのフレーム数
    }

    /// <summary>
    /// ミリ秒 → フレーム数への変換。
    /// </summary>
    private static int MillisToFrames(int milliseconds, int sampleRate)
    {
        if (milliseconds <= 0) return 0;
        double frames = (double)milliseconds * sampleRate / 1000.0;
        int result = (int)Math.Round(frames, MidpointRounding.AwayFromZero);
        return result > 0 ? result : 0;
    }

    /// <summary>
    /// 毎回違う乱数シードを生成する。
    /// </summary>
    private static Random CreateRandom()
    {
        // Guid と Tick を使って、比較的衝突しにくいシードを生成
        int seed = unchecked(Environment.TickCount ^ Guid.NewGuid().GetHashCode());
        return new Random(seed);
    }

    /// <summary>
    /// [minInclusive, maxInclusive] の long を一様乱数で生成する。
    /// </summary>
    private static long RandomBetweenLong(Random rng, long minInclusive, long maxInclusive)
    {
        if (minInclusive >= maxInclusive) return minInclusive;
        // Random.NextInt64(min, max) は max を含まないので +1 しておく
        return rng.NextInt64(minInclusive, maxInclusive + 1);
    }

    /// <summary>
    /// 長い音源を targetFrames に短縮するためのセグメントプラン (エ, オ) を構築する。
    /// </summary>
    private static bool PlanSegmentsForShorten(
        int totalFrames,
        int targetFrames,
        int mixFadeMsecs,
        int sampleRate,
        Random rng,
        out List<SegmentPlan> segments,
        out int fadeFrames)
    {
        segments = new List<SegmentPlan>();
        fadeFrames = 0;

        if (targetFrames <= 0 || totalFrames <= 0)
        {
            return false;
        }

        int fade = MillisToFrames(mixFadeMsecs, sampleRate);
        if (fade < 0) fade = 0;
        // クロスフェードが極端に大きいと不自然になるので、短縮後長さの半分を上限とする
        if (fade > targetFrames / 2)
        {
            fade = targetFrames / 2;
        }

        const double ratioMin = 0.7;
        double r = ratioMin / (1.0 + ratioMin); // ≒ 0.4118 (min / (min + max) の最小比)

        for (int iter = 0; iter < 8; iter++)
        {
            long fadeL = fade;
            long targetL = targetFrames;
            long sumLen = targetL + fadeL; // エ + オ = target + fade

            if (sumLen <= 2)
            {
                // 極端に短い場合は単純に先頭から targetFrames を切り出す
                segments = new List<SegmentPlan>
                {
                    new SegmentPlan { SourceStartFrame = 0, LengthFrames = targetFrames }
                };
                fadeFrames = 0;
                return true;
            }

            // 2 セグメント (エ, オ) の場合、min >= 0.7 * max を満たすための長さ範囲を計算
            long minPossibleLen = (long)Math.Ceiling(sumLen * r);
            long maxPossibleLen = (long)Math.Floor(sumLen * (1.0 - r));

            if (minPossibleLen < 1) minPossibleLen = 1;
            if (maxPossibleLen < minPossibleLen) maxPossibleLen = minPossibleLen;

            // 最短セグメントは少なくとも minPossibleLen 以上になるので、ここからフェードの上限を導く
            long theoreticalMinSegment = minPossibleLen;
            long maxFadeAllowed = theoreticalMinSegment / 10;

            // mixFadeMsecs の補正 (仕様: mixFadeMsecs は最小セグメント長の 1/10 以下に補正)
            if (fadeL > maxFadeAllowed)
            {
                fade = (int)maxFadeAllowed;
                if (fade < 0) fade = 0;
                // フェード時間を変更したので、同じロジックをやり直す
                continue;
            }

            // 実際に prefix (エ) の長さを決める。
            // ・sumLen から決まる min/max 範囲
            // ・各セグメント長は totalFrames を超えない
            long lower = Math.Max(minPossibleLen, sumLen - totalFrames); // オ が totalFrames を超えないための下限
            long upper = Math.Min(maxPossibleLen, totalFrames);          // エ が totalFrames を超えないための上限

            if (lower < 1) lower = 1;
            if (upper < lower)
            {
                // フェードが長すぎて条件を満たせないので、さらにフェードを短くして試す
                if (fade == 0)
                {
                    break;
                }
                fade /= 2;
                continue;
            }

            long prefixLen = RandomBetweenLong(rng, lower, upper);
            long suffixLen = sumLen - prefixLen;

            if (suffixLen < 1 || suffixLen > totalFrames)
            {
                // 理論上起きにくいが、安全のため再試行
                continue;
            }

            segments = new List<SegmentPlan>
            {
                // エ: sourceWav の先頭部分
                new SegmentPlan
                {
                    SourceStartFrame = 0,
                    LengthFrames = (int)prefixLen
                },
                // オ: sourceWav の末尾部分 (必ず末尾で終わる)
                new SegmentPlan
                {
                    SourceStartFrame = totalFrames - (int)suffixLen,
                    LengthFrames = (int)suffixLen
                }
            };

            fadeFrames = fade;
            return true;
        }

        // ここまでで満足なプランが作れなかった場合は、最後の手段として
        // 「先頭から targetFrames 分を切り出すだけ」の単純な短縮を行う (フェードなし)。
        segments = new List<SegmentPlan>
        {
            new SegmentPlan
            {
                SourceStartFrame = 0,
                LengthFrames = targetFrames
            }
        };
        fadeFrames = 0;
        return true;
    }

    /// <summary>
    /// 短い音源を targetFrames に伸長するためのセグメントプラン (ア, イ*, ウ) を構築する。
    /// </summary>
    private static bool PlanSegmentsForLengthen(
        int totalFrames,
        int targetFrames,
        int mixFadeMsecs,
        int sampleRate,
        Random rng,
        out List<SegmentPlan> segments,
        out int fadeFrames)
    {
        segments = null!;
        fadeFrames = 0;

        if (targetFrames <= totalFrames)
        {
            return false;
        }

        // 20% / 80% の位置 (仕様より)
        int pos20 = (int)(totalFrames * 20L / 100L);
        int pos80 = (int)(totalFrames * 80L / 100L);

        int middleMax = pos80 - pos20;
        if (middleMax <= 0)
        {
            // 真ん中部分を定義できないほど短い音源
            return false;
        }

        const int MaxMiddleSegments = 64; // 過剰なセグメント数にならないよう上限をおく

        for (int middleCount = 0; middleCount <= MaxMiddleSegments; middleCount++)
        {
            if (TryPlanSegmentsForLengthenCount(
                    middleCount,
                    totalFrames,
                    targetFrames,
                    mixFadeMsecs,
                    sampleRate,
                    pos20,
                    pos80,
                    rng,
                    out segments,
                    out fadeFrames))
            {
                return true;
            }
        }

        return false;
    }

    /// <summary>
    /// middleCount (イの個数) を指定して、ア + イ*middleCount + ウ の長さ配分と位置を決める。
    /// </summary>
    private static bool TryPlanSegmentsForLengthenCount(
        int middleCount,
        int totalFrames,
        int targetFrames,
        int mixFadeMsecs,
        int sampleRate,
        int pos20,
        int pos80,
        Random rng,
        out List<SegmentPlan> segments,
        out int fadeFrames)
    {
        segments = null!;
        fadeFrames = 0;

        int segmentCount = 2 + middleCount; // ア + イ*middleCount + ウ
        if (segmentCount <= 1)
        {
            return false;
        }

        int startMax = pos80;                 // sourceStart: 0%〜80%
        int middleMax = pos80 - pos20;        // sourceMiddle: 20%〜80%
        int endMax = totalFrames - pos20;     // sourceEnd: 20%〜100%

        if (startMax <= 0 || endMax <= 0)
        {
            return false;
        }

        if (middleCount > 0 && middleMax <= 0)
        {
            return false;
        }

        int[] maxLens = new int[segmentCount];
        maxLens[0] = startMax; // ア
        for (int i = 1; i < segmentCount - 1; i++)
        {
            maxLens[i] = middleCount > 0 ? middleMax : startMax;
        }
        maxLens[segmentCount - 1] = endMax;

        int globalMax = maxLens.Min();
        if (globalMax <= 0)
        {
            return false;
        }

        int fade = MillisToFrames(mixFadeMsecs, sampleRate);
        if (fade < 0) fade = 0;
        if (fade > targetFrames / 2)
        {
            // 極端なフェード長は避ける
            fade = targetFrames / 2;
        }

        // 数回フェード長を調整しつつ、長さ配分の乱数サンプリングを行う
        for (int iter = 0; iter < 8; iter++)
        {
            long fadeL = fade;
            long sumLen = targetFrames + fadeL * (segmentCount - 1);
            if (sumLen <= segmentCount)
            {
                return false;
            }

            double avgLen = sumLen / (double)segmentCount;
            if (avgLen > globalMax)
            {
                // どのように分配しても maxLens を超えてしまう
                return false;
            }

            long maxTotalByLimit = 0;
            for (int i = 0; i < segmentCount; i++) maxTotalByLimit += maxLens[i];
            if (sumLen > maxTotalByLimit)
            {
                // 各セグメントの最大値を全て使っても足りない場合は不可能
                return false;
            }

            bool fadeAdjustedAndRestart = false;

            for (int attempt = 0; attempt < 32; attempt++)
            {
                double[] weights = new double[segmentCount];
                double sumW = 0;
                for (int i = 0; i < segmentCount; i++)
                {
                    // ★注意★: 最短/最長の比が 0.7 以上になるよう、0.7〜1.0 の範囲で重みをとる。
                    weights[i] = 0.7 + 0.3 * rng.NextDouble();
                    sumW += weights[i];
                }

                double scale = sumLen / sumW;
                double[] lenD = new double[segmentCount];

                bool ok = true;
                for (int i = 0; i < segmentCount; i++)
                {
                    double v = weights[i] * scale;
                    if (v > maxLens[i])
                    {
                        // このウェイトでは指定されたセグメント最大長を超えてしまうのでやり直し
                        ok = false;
                        break;
                    }
                    lenD[i] = v;
                }

                if (!ok)
                {
                    continue;
                }

                int[] lengths = new int[segmentCount];
                double[] frac = new double[segmentCount];
                long sumInt = 0;
                for (int i = 0; i < segmentCount; i++)
                {
                    double d = lenD[i];
                    int li = (int)Math.Floor(d);
                    if (li < 1) li = 1;
                    if (li > maxLens[i]) li = maxLens[i];
                    lengths[i] = li;
                    sumInt += li;
                    frac[i] = d - li;
                }

                long delta = sumLen - sumInt;
                if (delta > 0)
                {
                    // 小数部の大きい順に 1 フレームずつ足していき、合計を合わせる
                    int[] order = Enumerable.Range(0, segmentCount)
                                            .OrderByDescending(i => frac[i])
                                            .ToArray();
                    int idx = 0;
                    while (delta > 0 && idx < order.Length)
                    {
                        int iSeg = order[idx];
                        if (lengths[iSeg] < maxLens[iSeg])
                        {
                            lengths[iSeg]++;
                            delta--;
                        }
                        else
                        {
                            idx++;
                        }
                    }
                }
                else if (delta < 0)
                {
                    // 逆に長さを削る必要がある場合は、小数部の小さい順から削る
                    int[] order = Enumerable.Range(0, segmentCount)
                                            .OrderBy(i => frac[i])
                                            .ToArray();
                    int idx = 0;
                    while (delta < 0 && idx < order.Length)
                    {
                        int iSeg = order[idx];
                        if (lengths[iSeg] > 1)
                        {
                            lengths[iSeg]--;
                            delta++;
                        }
                        else
                        {
                            idx++;
                        }
                    }
                }

                long finalSum = 0;
                for (int i = 0; i < segmentCount; i++) finalSum += lengths[i];
                if (finalSum != sumLen)
                {
                    // 合計が合わなければこの試行は失敗
                    continue;
                }

                int minLen = lengths.Min();
                int maxLen = lengths.Max();
                if (minLen <= 0 || (double)minLen / maxLen < 0.7)
                {
                    // 最短/最長比が 0.7 を下回る場合もやり直し
                    continue;
                }

                if (fade > 0)
                {
                    int maxFadeAllowed = minLen / 10;
                    if (maxFadeAllowed <= 0)
                    {
                        // フェードを全く取れないような極端な短さなら、フェード自体を 0 にする
                        fade = 0;
                        fadeAdjustedAndRestart = true;
                        break;
                    }

                    if (fade > maxFadeAllowed)
                    {
                        // フェードが長すぎるので、最小セグメント長/10 に合わせて短くする
                        fade = maxFadeAllowed;
                        fadeAdjustedAndRestart = true;
                        break;
                    }
                }

                // ここまで来れば lengths と fade は条件を満たしているので、
                // 実際のセグメント位置 (ア, イ*, ウ) を元音源上に割り当てる。
                segments = new List<SegmentPlan>(segmentCount);

                // ア: sourceStart の先頭部分 (0% からの prefix)
                segments.Add(new SegmentPlan
                {
                    SourceStartFrame = 0,
                    LengthFrames = lengths[0]
                });

                // イ: sourceMiddle の任意位置から一部を取り出す (middleCount 個)
                for (int m = 0; m < middleCount; m++)
                {
                    int length = lengths[1 + m];
                    int available = middleMax - length;
                    int offset = 0;
                    if (available > 0)
                    {
                        // 20%〜80% の範囲内で length 分確保できるような開始オフセットをランダムに決める
                        offset = (int)rng.NextInt64(0, available + 1);
                    }

                    segments.Add(new SegmentPlan
                    {
                        SourceStartFrame = pos20 + offset,
                        LengthFrames = length
                    });
                }

                // ウ: sourceEnd の末尾部分 (必ず原曲の末尾で終わる suffix)
                int lastLen = lengths[segmentCount - 1];
                segments.Add(new SegmentPlan
                {
                    SourceStartFrame = totalFrames - lastLen,
                    LengthFrames = lastLen
                });

                fadeFrames = fade;
                return true;
            }

            if (fadeAdjustedAndRestart)
            {
                // フェード長を変更したので、同じ middleCount で再度 sumLen から計算し直す
                continue;
            }

            // 乱数を変えても length 分配がうまくいかない場合は、フェードをさらに短くして再試行
            if (fade == 0)
            {
                return false;
            }

            fade /= 2;
        }

        return false;
    }

    /// <summary>
    /// セグメントプランに基づき、クロスフェード付きで最終的な波形を構築する。
    /// </summary>
    private static float[] RenderSegments(
        IReadOnlyList<SegmentPlan> segments,
        float[] sourceSamples,
        int channels,
        int fadeFrames,
        int targetFrames)
    {
        if (segments == null) throw new ArgumentNullException(nameof(segments));
        if (segments.Count == 0) throw new ArgumentException("segments が空です。", nameof(segments));
        if (channels <= 0) throw new ArgumentOutOfRangeException(nameof(channels));

        int segmentCount = segments.Count;
        int[] startFrames = new int[segmentCount];

        // クロスフェードを考慮した開始位置の計算
        // start[0] = 0
        // start[i] = start[i-1] + len[i-1] - fadeFrames
        // こうすることで、fadeFrames 分だけ前後が重なって再生される。
        startFrames[0] = 0;
        for (int i = 1; i < segmentCount; i++)
        {
            startFrames[i] = startFrames[i - 1] + segments[i - 1].LengthFrames - fadeFrames;
        }

        // 仕様上、出力長は必ず targetFrames にする
        int finalFrames = targetFrames;
        float[] output = new float[finalFrames * channels];

        int fadeFramesUsed = Math.Max(0, fadeFrames);

        // フェードなし or セグメント1個のときは単純な連結
        if (fadeFramesUsed == 0 || segmentCount == 1)
        {
            for (int si = 0; si < segmentCount; si++)
            {
                var seg = segments[si];
                int segStart = startFrames[si];
                for (int f = 0; f < seg.LengthFrames; f++)
                {
                    int destFrame = segStart + f;
                    if (destFrame < 0 || destFrame >= finalFrames)
                        continue;

                    int srcFrame = seg.SourceStartFrame + f;
                    for (int ch = 0; ch < channels; ch++)
                    {
                        int srcIndex = srcFrame * channels + ch;
                        int dstIndex = destFrame * channels + ch;
                        float sample = (srcIndex >= 0 && srcIndex < sourceSamples.Length)
                            ? sourceSamples[srcIndex]
                            : 0f;
                        output[dstIndex] += sample;
                    }
                }
            }

            return output;
        }

        int fadeDen = fadeFramesUsed > 1 ? fadeFramesUsed - 1 : 1;

        // ★注意★: クロスフェード部分では「前セグメントのフェードアウト」と
        // 「後セグメントのフェードイン」が同一サンプル位置で重なるよう、
        // startFrames と gain 計算をきちんと同期させる必要がある。
        for (int si = 0; si < segmentCount; si++)
        {
            var seg = segments[si];
            int segStart = startFrames[si];
            int segLen = seg.LengthFrames;

            for (int f = 0; f < segLen; f++)
            {
                int destFrame = segStart + f;
                if (destFrame < 0 || destFrame >= finalFrames)
                    continue;

                double gain = 1.0;

                // 先頭側フェードイン (最初のセグメント以外)
                if (si > 0 && f < fadeFramesUsed)
                {
                    double t = f / (double)fadeDen; // 0 → 1
                    gain *= t;
                }

                // 末尾側フェードアウト (最後のセグメント以外)
                if (si < segmentCount - 1 && f >= segLen - fadeFramesUsed)
                {
                    int posInFade = f - (segLen - fadeFramesUsed);
                    double t = (fadeDen - posInFade) / (double)fadeDen; // 1 → 0
                    if (t < 0) t = 0;
                    gain *= t;
                }

                int srcFrame = seg.SourceStartFrame + f;
                for (int ch = 0; ch < channels; ch++)
                {
                    int srcIndex = srcFrame * channels + ch;
                    int dstIndex = destFrame * channels + ch;
                    float sample = (srcIndex >= 0 && srcIndex < sourceSamples.Length)
                        ? sourceSamples[srcIndex]
                        : 0f;
                    output[dstIndex] += (float)(sample * gain);
                }
            }
        }

        return output;
    }

    /// <summary>
    /// float[-1,1] 配列を 16bit PCM little-endian のバイト配列に変換する。
    /// </summary>
    private static byte[] ConvertFloatToPcm16(float[] samples)
    {
        if (samples == null) throw new ArgumentNullException(nameof(samples));

        byte[] bytes = new byte[samples.Length * 2];
        Span<byte> span = bytes.AsSpan();

        for (int i = 0; i < samples.Length; i++)
        {
            float v = samples[i];
            if (v > 1.0f) v = 1.0f;
            if (v < -1.0f) v = -1.0f;

            short s = (short)Math.Round(v * short.MaxValue, MidpointRounding.AwayFromZero);
            BinaryPrimitives.WriteInt16LittleEndian(span.Slice(i * 2, 2), s);
        }

        return bytes;
    }

    /// <summary>
    /// 16bit PCM データから最小限の RIFF/WAVE ヘッダを生成し、完全な WAV バイト列を構築する。
    /// </summary>
    private static byte[] BuildPcm16Wave(byte[] pcmData, int sampleRate, int channels)
    {
        if (pcmData == null) throw new ArgumentNullException(nameof(pcmData));

        const short audioFormatPcm = 1;
        const short bitsPerSample = 16;
        short blockAlign = (short)(channels * (bitsPerSample / 8));
        int byteRate = sampleRate * blockAlign;
        int subchunk2Size = pcmData.Length;
        int chunkSize = 36 + subchunk2Size; // 4("WAVE") + (8+fmt) + (8+data)

        byte[] result = new byte[44 + pcmData.Length];
        Span<byte> span = result.AsSpan();

        // RIFF ヘッダ
        Encoding.ASCII.GetBytes("RIFF").CopyTo(span.Slice(0, 4));
        BinaryPrimitives.WriteInt32LittleEndian(span.Slice(4, 4), chunkSize);
        Encoding.ASCII.GetBytes("WAVE").CopyTo(span.Slice(8, 4));

        // fmt チャンク
        Encoding.ASCII.GetBytes("fmt ").CopyTo(span.Slice(12, 4));
        BinaryPrimitives.WriteInt32LittleEndian(span.Slice(16, 4), 16);                  // サブチャンク1サイズ (PCM 固定)
        BinaryPrimitives.WriteInt16LittleEndian(span.Slice(20, 2), audioFormatPcm);      // フォーマット ID (PCM)
        BinaryPrimitives.WriteInt16LittleEndian(span.Slice(22, 2), (short)channels);     // チャンネル数
        BinaryPrimitives.WriteInt32LittleEndian(span.Slice(24, 4), sampleRate);          // サンプリングレート
        BinaryPrimitives.WriteInt32LittleEndian(span.Slice(28, 4), byteRate);            // バイトレート
        BinaryPrimitives.WriteInt16LittleEndian(span.Slice(32, 2), blockAlign);          // ブロックアライン
        BinaryPrimitives.WriteInt16LittleEndian(span.Slice(34, 2), bitsPerSample);       // 量子化ビット数

        // data チャンク
        Encoding.ASCII.GetBytes("data").CopyTo(span.Slice(36, 4));
        BinaryPrimitives.WriteInt32LittleEndian(span.Slice(40, 4), subchunk2Size);

        // PCM データ本体
        Buffer.BlockCopy(pcmData, 0, result, 44, pcmData.Length);

        return result;
    }

    #endregion
}



// 以下のコード By ChatGPT
public static class AiWaveMixUtil
{
    /// <summary>
    /// 44.1kHz・2ch・16bit PCM（リトルエンディアン）データを前提とした、波形合成（ミキシング）処理を行う。
    /// targetWavRandomAccess 上の PCM データの一部に、sourceWav の PCM データをフェードイン/アウト付きで加算合成して書き戻す。
    /// </summary>
    /// <remarks>
    /// ・targetWavRandomAccess は数十GB以上を想定し、アクセス位置は long で扱う。
    /// ・高速化のため、対象範囲を ReadRandomAsync で一度だけ読み、Span 上で演算し、WriteRandomAsync で一度だけ書き戻す。
    /// ・sourceWav は最大でも 1.5GB 程度（int に収まる）を想定する。
    /// </remarks>
    /// <param name="targetWavRandomAccess">編集対象の Wave 生データへのランダムアクセス I/F（入出力）</param>
    /// <param name="sourceWav">合成元の Wave 生データ（ReadOnlyMemory&lt;byte&gt;）</param>
    /// <param name="targetPosition">target 内の合成開始位置（秒）</param>
    /// <param name="sourceWavPosition">source 内の使用開始位置（秒）</param>
    /// <param name="length">合成する長さ（秒）</param>
    /// <param name="deltaLeft">左チャンネルの dB 音量調整（正で増幅、負で減衰）</param>
    /// <param name="deltaRight">右チャンネルの dB 音量調整（正で増幅、負で減衰）</param>
    /// <param name="fadein">フェードイン長（秒）</param>
    /// <param name="fadeout">フェードアウト長（秒）</param>
    public static void MixWaveData(
        IRandomAccess<byte> targetWavRandomAccess,
        ReadOnlyMemory<byte> sourceWav,
        double targetPosition,
        double sourceWavPosition,
        double length,
        double deltaLeft,
        double deltaRight,
        double fadein,
        double fadeout)
    {
        // 波形フォーマット前提値（元実装と同一）
        const int sampleRate = 44100;
        const int channels = 2;
        const int bitsPerSample = 16;

        // 1サンプル(1ch)あたりのバイト数（16bit => 2bytes）
        const int bytesPerSample = bitsPerSample / 8;
        // 1フレーム(2ch合計)あたりのバイト数（L(2) + R(2) => 4bytes）
        const int blockAlign = channels * bytesPerSample;

        if (targetWavRandomAccess is null)
        {
            throw new ArgumentNullException(nameof(targetWavRandomAccess));
        }

        if (targetPosition < 0 || sourceWavPosition < 0 || length <= 0)
        {
            // 負の位置や長さ0以下なら何もしない（元実装踏襲）
            return;
        }

        // 合成を開始するフレーム位置（整数に丸め：元実装と同じキャスト）
        long targetStartFrame = (long)(targetPosition * sampleRate);
        long sourceStartFrame = (long)(sourceWavPosition * sampleRate);
        if (targetStartFrame < 0 || sourceStartFrame < 0)
        {
            return;
        }

        // 合成するフレーム数（元実装と同じ）
        long requestedFrames = (long)(length * sampleRate);
        if (requestedFrames <= 0)
        {
            return;
        }

        // バイト単位での開始位置
        long targetStartByte = targetStartFrame * blockAlign;
        long sourceStartByte = sourceStartFrame * blockAlign;

        if (targetStartByte < 0 || sourceStartByte < 0)
        {
            return;
        }

        // target のファイルサイズを取得（refresh は常に false 指定）
        long targetFileSize = targetWavRandomAccess
            .GetFileSizeAsync(refresh: false, cancel: CancellationToken.None)
            .GetAwaiter()
            .GetResult();

        if (targetFileSize <= 0)
        {
            return;
        }

        // 範囲外チェック（target）
        long targetRemBytes = targetFileSize - targetStartByte;
        if (targetRemBytes <= 0)
        {
            return;
        }

        // 範囲外チェック（source：sourceWav は int 長）
        if (sourceStartByte >= sourceWav.Length)
        {
            return;
        }

        long sourceRemBytes = (long)sourceWav.Length - sourceStartByte;
        if (sourceRemBytes <= 0)
        {
            return;
        }

        // 実際に合成できる最大フレーム数を決定（target は long、source は int に収まる前提）
        long maxTargetFrames = targetRemBytes / blockAlign;
        long maxSourceFrames = sourceRemBytes / blockAlign;

        long framesToProcess = requestedFrames;
        if (framesToProcess > maxTargetFrames) framesToProcess = maxTargetFrames;
        if (framesToProcess > maxSourceFrames) framesToProcess = maxSourceFrames;

        if (framesToProcess <= 0)
        {
            return;
        }

        // ここで bytesToProcess は source 側制約により int に収まる想定だが、防御的にチェックする
        long bytesToProcessLong = framesToProcess * blockAlign;
        if (bytesToProcessLong > int.MaxValue)
        {
            // このケースは「source が int である」前提だと通常起きないが、想定外の巨大要求を明示的に拒否
            throw new InvalidOperationException("処理対象が大きすぎます（メモリ上に展開できません）。");
        }

        int bytesToProcess = (int)bytesToProcessLong;

        // source 側は int インデックスでスライスする必要があるため、ここで検証
        if (sourceStartByte > int.MaxValue)
        {
            // sourceWav は 1.5GB 程度（int に収まる）前提なので通常起きないが、防御
            return;
        }

        // --------------------------------------------------------------------
        // 1) source の必要部分だけをコピー（元実装と同様に一時配列化）
        // --------------------------------------------------------------------
        ReadOnlyMemory<byte> sourceSlice = sourceWav.Slice((int)sourceStartByte, bytesToProcess);
        byte[] sourceBytes = sourceSlice.ToArray(); // source は小さめ前提

        // --------------------------------------------------------------------
        // 2) target の必要部分だけを「一度だけ」ReadRandomAsync で読み出し
        //    ※ 指示どおり、ReadRandomAsync は全体で1回のみ
        // --------------------------------------------------------------------
        byte[] targetBytes = new byte[bytesToProcess];

        int bytesRead = targetWavRandomAccess
            .ReadRandomAsync(targetStartByte, targetBytes, CancellationToken.None)
            .GetAwaiter()
            .GetResult();

        if (bytesRead != bytesToProcess)
        {
            // 指示：意図しているバイト数と異なる場合は例外
            throw new IOException($"ReadRandomAsync の戻り値が想定外です。expected={bytesToProcess}, actual={bytesRead}");
        }

        // --------------------------------------------------------------------
        // 3) Span 上でフェード + dB スケーリング + 合成（高速化の核）
        //    ・byte[] を short の Span にキャストして per-sample 演算を高速化
        //    ・16bit PCM（リトルエンディアン）前提
        // --------------------------------------------------------------------
        if ((bytesToProcess & 1) != 0)
        {
            // 16bit PCM なので偶数バイトであるべき
            throw new InvalidOperationException("処理対象バイト数が 16bit PCM として不正です。");
        }

        // byte[] -> Span<short>（各サンプル単位：L,R が交互に並ぶ）
        Span<short> sourceSamples = MemoryMarshal.Cast<byte, short>(sourceBytes);
        Span<short> targetSamples = MemoryMarshal.Cast<byte, short>(targetBytes);

        int frameCount = checked((int)framesToProcess);

        float amplitudeFactorLeft = (float)Math.Pow(10.0, deltaLeft / 20.0);   // dB -> 倍率
        float amplitudeFactorRight = (float)Math.Pow(10.0, deltaRight / 20.0); // dB -> 倍率
        double totalDurationSec = frameCount / (double)sampleRate;

        // 3-1) source 側にフェード & 音量調整を適用（元実装と同じ考え方）
        for (int i = 0; i < frameCount; i++)
        {
            double t = i / (double)sampleRate;

            // フェードイン（0 -> 1）
            double fi = 1.0;
            if (fadein > 0.0 && t < fadein)
            {
                fi = t / fadein;
            }

            // フェードアウト（1 -> 0）
            double fo = 1.0;
            double timeFromEnd = totalDurationSec - t;
            if (fadeout > 0.0 && timeFromEnd < fadeout)
            {
                fo = timeFromEnd / fadeout;
            }

            float fadeFactor = (float)(fi * fo);

            int leftIndex = i * 2;
            int rightIndex = leftIndex + 1;

            float scaledLeft = sourceSamples[leftIndex] * amplitudeFactorLeft * fadeFactor;
            float scaledRight = sourceSamples[rightIndex] * amplitudeFactorRight * fadeFactor;

            // クリップして格納（short 範囲）
            sourceSamples[leftIndex] = (short)Math.Clamp((int)scaledLeft, short.MinValue, short.MaxValue);
            sourceSamples[rightIndex] = (short)Math.Clamp((int)scaledRight, short.MinValue, short.MaxValue);
        }

        // 3-2) target に加算合成（クリップ付き）
        for (int i = 0; i < frameCount; i++)
        {
            int leftIndex = i * 2;
            int rightIndex = leftIndex + 1;

            int mixedLeft = targetSamples[leftIndex] + sourceSamples[leftIndex];
            int mixedRight = targetSamples[rightIndex] + sourceSamples[rightIndex];

            targetSamples[leftIndex] = (short)Math.Clamp(mixedLeft, short.MinValue, short.MaxValue);
            targetSamples[rightIndex] = (short)Math.Clamp(mixedRight, short.MinValue, short.MaxValue);
        }

        // --------------------------------------------------------------------
        // 4) 書き戻し：WriteRandomAsync を「一度だけ」呼ぶ
        // --------------------------------------------------------------------
        targetWavRandomAccess
            .WriteRandomAsync(targetStartByte, targetBytes, CancellationToken.None)
            .GetAwaiter()
            .GetResult();
    }


    /// <summary>
    /// 44.1kHz・2ch・16bit PCM データを前提とした、メモリ上での波形合成処理を行う。
    /// targetWav に、sourceWav をミキシングした結果を書き込む。
    /// </summary>
    /// <param name="targetWav">編集対象の Wave 生データ (Memory&lt;byte&gt;)</param>
    /// <param name="sourceWav">合成元の Wave 生データ (ReadOnlyMemory&lt;byte&gt;)</param>
    /// <param name="targetPosition">targetWav 内の合成開始位置(秒)</param>
    /// <param name="sourceWavPosition">sourceWav 内の使用開始位置(秒)</param>
    /// <param name="length">合成する長さ(秒)</param>
    /// <param name="delta">dB 単位の音量調整値(正で音量アップ、負でダウン)</param>
    /// <param name="fadein">フェードインの長さ(秒)</param>
    /// <param name="fadeout">フェードアウトの長さ(秒)</param>
    public static void MixWaveData_Old(
        Memory<byte> targetWav,
        ReadOnlyMemory<byte> sourceWav,
        double targetPosition,
        double sourceWavPosition,
        double length,
        double deltaLeft,
        double deltaRight,
        double fadein,
        double fadeout)
    {
        // 波形フォーマット前提値
        const int sampleRate = 44100;
        const int channels = 2;
        const int bitsPerSample = 16;
        // 1サンプル(1ch)あたりのバイト数
        const int bytesPerSample = bitsPerSample / 8; // = 2
                                                      // 1フレーム(全ch合計)あたりのバイト数(2ch前提)
        const int blockAlign = channels * bytesPerSample; // 4バイト

        if (targetPosition < 0 || sourceWavPosition < 0 || length <= 0)
        {
            // 負の位置や長さ0以下なら何もしない
            return;
        }

        // 合成を開始するフレーム位置(整数)
        long targetStartFrame = (long)(targetPosition * sampleRate);
        long sourceStartFrame = (long)(sourceWavPosition * sampleRate);
        if (targetStartFrame < 0 || sourceStartFrame < 0)
        {
            // もし計算結果が負なら処理しない
            return;
        }

        // 合成するフレーム数(整数)
        long framesToProcess = (long)(length * sampleRate);
        if (framesToProcess <= 0)
        {
            return;
        }

        // バイト単位での開始位置
        long targetStartByte = targetStartFrame * blockAlign;
        long sourceStartByte = sourceStartFrame * blockAlign;

        // targetWav / sourceWav の残りバイト数
        long targetRemBytes = targetWav.Length - targetStartByte;
        long sourceRemBytes = sourceWav.Length - sourceStartByte;

        // 範囲外チェック
        if (targetRemBytes <= 0 || sourceRemBytes <= 0)
        {
            // 合成開始位置がどちらか一方でも範囲外なら何もしない
            return;
        }

        // 実際に合成できる最大フレーム数を決定
        long maxTargetFrames = targetRemBytes / blockAlign;
        long maxSourceFrames = sourceRemBytes / blockAlign;
        if (framesToProcess > maxTargetFrames)
            framesToProcess = maxTargetFrames;
        if (framesToProcess > maxSourceFrames)
            framesToProcess = maxSourceFrames;

        if (framesToProcess <= 0)
        {
            return;
        }

        // 実際に処理するバイト数
        int bytesToProcess = (int)(framesToProcess * blockAlign);

        // --------------------------------------------------------------------
        // 1) sourceWav の必要部分だけを一時バッファへコピー
        // --------------------------------------------------------------------
        // メモリコピー(高速化のため必要分のみ)
        ReadOnlyMemory<byte> sourceSlice = sourceWav.Slice((int)sourceStartByte, bytesToProcess);
        // short 単位に処理しやすいよう変換用バッファ
        byte[] sourceBytes = sourceSlice.ToArray();

        // --------------------------------------------------------------------
        // 2) フェードイン・フェードアウト・dBスケーリング適用
        // --------------------------------------------------------------------
        // short[] に変換しておいて、サンプルごとに処理
        short[] sourceSamples = new short[sourceBytes.Length / 2];
        Buffer.BlockCopy(sourceBytes, 0, sourceSamples, 0, sourceBytes.Length);

        float amplitudeFactorLeft = (float)Math.Pow(10.0, deltaLeft / 20.0);  // dB → 倍率
        float amplitudeFactorRight = (float)Math.Pow(10.0, deltaRight / 20.0);  // dB → 倍率
        double totalDurationSec = framesToProcess / (double)sampleRate;

        for (int i = 0; i < framesToProcess; i++)
        {
            // フレーム i に対応する時間(秒)
            double t = i / (double)sampleRate;

            // フェードイン率
            double fi = 1.0;
            if (fadein > 0.0 && t < fadein)
            {
                fi = t / fadein;  // 0→1 に上昇
            }

            // フェードアウト率
            double fo = 1.0;
            double timeFromEnd = totalDurationSec - t; // 終了まで残り秒
            if (fadeout > 0.0 && timeFromEnd < fadeout)
            {
                fo = timeFromEnd / fadeout; // 1→0 に減少
            }

            // フェード率の合成
            float fadeFactor = (float)(fi * fo);

            // 左(0), 右(1) と 2chぶん連続している想定
            int leftIndex = i * 2;
            int rightIndex = i * 2 + 1;

            // スケーリング（dB + フェードイン/アウト）
            float scaledLeft = sourceSamples[leftIndex] * amplitudeFactorLeft * fadeFactor;
            float scaledRight = sourceSamples[rightIndex] * amplitudeFactorRight * fadeFactor;

            // short 範囲にクリップ
            sourceSamples[leftIndex] = (short)Math.Clamp((int)scaledLeft, short.MinValue, short.MaxValue);
            sourceSamples[rightIndex] = (short)Math.Clamp((int)scaledRight, short.MinValue, short.MaxValue);
        }

        // --------------------------------------------------------------------
        // 3) targetWav に合成（ミキシング）
        // --------------------------------------------------------------------
        Span<byte> targetSpan = targetWav.Slice((int)targetStartByte, bytesToProcess).Span;

        for (int i = 0; i < framesToProcess; i++)
        {
            int bytePos = i * blockAlign;

            // target の左 ch
            short targetLeft = BitConverter.ToInt16(targetSpan.Slice(bytePos, 2));
            // target の右 ch
            short targetRight = BitConverter.ToInt16(targetSpan.Slice(bytePos + 2, 2));

            // source の左 ch
            short sourceLeft = sourceSamples[i * 2];
            // source の右 ch
            short sourceRight = sourceSamples[i * 2 + 1];

            // 加算 (オーバーフロー対策で一旦 int へ)
            int mixedLeft = targetLeft + sourceLeft;
            int mixedRight = targetRight + sourceRight;

            // short の範囲にクリップ
            mixedLeft = Math.Clamp(mixedLeft, short.MinValue, short.MaxValue);
            mixedRight = Math.Clamp(mixedRight, short.MinValue, short.MaxValue);

            // 書き戻し
            BitConverter.TryWriteBytes(targetSpan.Slice(bytePos, 2), (short)mixedLeft);
            BitConverter.TryWriteBytes(targetSpan.Slice(bytePos + 2, 2), (short)mixedRight);
        }
    }
}

public static class AiWaveUtil
{
    /// <summary>
    /// 指定された WaveFormat / 長さ(duration秒) / レベル(db dB) の微小ノイズ(生PCMデータ)を生成する
    /// </summary>
    /// <param name="waveFormat">使用したい WaveFormat (サンプリングレート, ビット深度など)</param>
    /// <param name="duration">生成するデータの長さ(秒)</param>
    /// <param name="db">ノイズの音量(dB)。例: -80.0</param>
    /// <returns>wav ヘッダなしのPCMデータ(byte配列)</returns>
    public static byte[] GenerateSilentNoiseData(double durationSeconds, WaveFormat format, double dbFS = -80.0)
    {
        if (dbFS >= 0) throw new ArgumentOutOfRangeException(
            nameof(dbFS), "dbFS は 0 より小さい必要があります (0dBFS がフルスケール)");

        // dB → 振幅係数 (0.0001 ≒ -80 dBFS)
        double amp = Math.Pow(10.0, dbFS / 20.0);

        int totalFrames = (int)Math.Round(format.SampleRate * durationSeconds);
        int bufferSize = totalFrames * format.BlockAlign;
        byte[] buffer = new byte[bufferSize];

        Random rng = new Random();                 // 高度な用途なら RNGCryptoServiceProvider などでも可
        int offset = 0;

        for (int frame = 0; frame < totalFrames; frame++)
        {
            for (int ch = 0; ch < format.Channels; ch++)
            {
                double sample = (rng.NextDouble() * 2.0 - 1.0) * amp; // -1.0〜+1.0 にスケール後、振幅係数を掛ける

                switch (format.Encoding)
                {
                    case WaveFormatEncoding.IeeeFloat:
                        float f = (float)sample;                // +/-1.0 がフルスケール
                        BitConverter.GetBytes(f)
                                    .CopyTo(buffer, offset);
                        offset += 4;
                        break;

                    case WaveFormatEncoding.Pcm:
                        switch (format.BitsPerSample)
                        {
                            case 16:
                                short s16 = (short)(sample * short.MaxValue);
                                BitConverter.GetBytes(s16).CopyTo(buffer, offset);
                                offset += 2;
                                break;

                            case 24:
                                int s24 = (int)(sample * 8_388_607); // 2^23-1
                                buffer[offset++] = (byte)(s24 & 0xFF);
                                buffer[offset++] = (byte)((s24 >> 8) & 0xFF);
                                buffer[offset++] = (byte)((s24 >> 16) & 0xFF);
                                break;

                            case 32:
                                int s32 = (int)(sample * int.MaxValue);
                                BitConverter.GetBytes(s32).CopyTo(buffer, offset);
                                offset += 4;
                                break;

                            case 8:
                                // 8-bit PCM は 0–255 の unsigned。128 が 0。
                                byte s8 = (byte)(128 + sample * 127);
                                buffer[offset++] = s8;
                                break;

                            default:
                                throw new NotSupportedException(
                                    $"PCM {format.BitsPerSample}-bit には未対応です");
                        }
                        break;

                    default:
                        throw new NotSupportedException(
                            $"Encoding {format.Encoding} には未対応です");
                }
            }
        }
        return buffer;
    }
    /// <summary>
    /// Box-Muller 法で N(0,1) のガウシアン乱数を1つ生成する。
    /// </summary>
    private static double NextGaussian(Random rand)
    {
        // 2つの一様乱数 u1, u2 を用意 (0 ~ 1)
        double u1 = 1.0 - rand.NextDouble(); // 0 は引かないようにする (logの発散回避)
        double u2 = 1.0 - rand.NextDouble();
        // Box-Muller transform
        double radius = Math.Sqrt(-2.0 * Math.Log(u1));
        double theta = 2.0 * Math.PI * u2;
        double z = radius * Math.Sin(theta);
        return z;
    }


    public static double CalcWavDurationFromSizeInByte(long size, WaveFormat format)
    {
        return (double)size / (double)format.AverageBytesPerSecond;
    }

    public static int GetWavDataSizeInByteFromTime(double timeSecs, WaveFormat format)
    {
        double exact = timeSecs * format.AverageBytesPerSecond;

        int byteOffset = (int)Math.Round(exact);
        byteOffset -= byteOffset % format.BlockAlign; // 必ずフレーム境界に
        return byteOffset;
    }

    public static long GetWavDataPositionInByteFromTime(double timeSecs, WaveFormat format)
    {
        double exact = timeSecs * format.AverageBytesPerSecond;

        long byteOffset = (long)Math.Round(exact);
        byteOffset -= byteOffset % format.BlockAlign; // 必ずフレーム境界に
        return byteOffset;
    }

    /// <summary>
    /// 1. float[] -> 16bit PCM (ステレオ 44.1kHz) の生データ (ヘッダなし) を表す Memory<byte> に変換する。
    /// </summary>
    /// <param name="floatBuffer">
    ///     NAudio の ISampleProvider.Read() などで取得したインターリーブ済みステレオの float 配列。
    ///     サンプルレート 44.1kHz, 2ch, フォーマットは -1.0f ～ +1.0f を想定。
    /// </param>
    /// <param name="cancel">巨大なループを回す際のキャンセル用トークン</param>
    /// <returns>16bit PCM (ステレオ) の生バイトデータ (WAV ヘッダを含まない波形部分のみ)</returns>
    public static Memory<byte> ConvertFloatArrayToPcm16Memory(
        float[] floatBuffer,
        CancellationToken cancel
    )
    {
        if (floatBuffer == null) throw new ArgumentNullException(nameof(floatBuffer));

        // 16ビットPCM (2バイト) x サンプル数 だけのバイト配列を確保する。
        // float配列はステレオ(2ch)をインターリーブしているが、変換は単純に
        // 「各 float サンプルを 16bit(short) にする」だけなので、
        // サンプル数 x 2 バイト = バイト数になる。
        // （チャンネル数は気にせず、単純にサンプルの数だけループを回せばよい）
        int totalSamples = floatBuffer.Length;
        int totalBytes = totalSamples * sizeof(short); // short = 2 bytes
        var byteArray = new byte[totalBytes];

        // NAudio.WaveBuffer を使うと、ShortBuffer[i] を簡単に扱える
        // (内部的には GCHandle でバッファを Pin してくれる)。
        var waveBuffer = new WaveBuffer(byteArray);

        for (int i = 0; i < totalSamples; i++)
        {
            // 適宜キャンセル要求をチェック (大きい配列を想定するなら頻度調整)
            if ((i % 10000) == 0)
            {
                cancel.ThrowIfCancellationRequested();
            }

            // float(-1.0～+1.0) を 16bit PCM(-32768～32767) に変換
            // ここでは Math.Round で四捨五入後にキャストする例。
            // (NAudio の規定実装では単純に (short)(floatVal * 32767) などがよく使われる)
            float sampleFloat = floatBuffer[i];

            // クリッピングは適宜必要に応じて実装
            if (sampleFloat > 1.0f) sampleFloat = 1.0f;
            if (sampleFloat < -1.0f) sampleFloat = -1.0f;

            waveBuffer.ShortBuffer[i] = (short)Math.Round(sampleFloat * 32767.0f);
        }

        // 返り値は Memory<byte> にして返す
        return new Memory<byte>(byteArray);
    }

    /// <summary>
    /// 2. 16bit PCM (ステレオ 44.1kHz) の生データ (ヘッダなし) -> float[] に変換する。
    /// </summary>
    /// <param name="pcmData">
    ///     「44.1kHz / 2ch / 16bit PCM」の生データ (WAVヘッダなし)。
    ///     ステレオの場合、各サンプルがインターリーブされている。
    /// </param>
    /// <param name="cancel">巨大なループを回す際のキャンセル用トークン</param>
    /// <returns>変換後の float[] (インターリーブ済みステレオのサンプル列)</returns>
    public static float[] ConvertPcm16ToFloatArray(
        ReadOnlyMemory<byte> pcmData,
        CancellationToken cancel
    )
    {
        // byte -> short に変換するので、2 バイトで 1サンプル
        // ステレオかどうかにかかわらず、単純にトータルサンプル数 = (バイト数 / 2)。
        // たとえば「2ch 16bit」でも、(バイト配列の合計長)/2 が short の個数になる。
        // ステレオなら、左サンプル / 右サンプル が交互に格納されている、というだけ。
        int totalBytes = pcmData.Length;
        if (totalBytes % 2 != 0)
        {
            throw new ArgumentException("PCMデータが 2 バイト単位になっていません。壊れている可能性があります。");
        }
        int totalSamples = totalBytes / sizeof(short);

        // 変換先 float 配列
        float[] floatArray = new float[totalSamples];

        // NAudio.WaveBuffer は ReadOnlyMemory<byte> から直接作れないため、
        // 必要ならば一旦 ToArray() する。サイズが巨大な場合は別途工夫が要る。
        var byteArray = pcmData.ToArray();
        var waveBuffer = new WaveBuffer(byteArray);

        for (int i = 0; i < totalSamples; i++)
        {
            if ((i % 10000) == 0)
            {
                cancel.ThrowIfCancellationRequested();
            }

            // short(-32768～32767) -> float(-1.0～+1.0 近辺)
            short sampleShort = waveBuffer.ShortBuffer[i];
            float sampleFloat = sampleShort / 32768.0f; // -1.0 ～ 約+0.9999
            floatArray[i] = sampleFloat;
        }

        return floatArray;
    }
}

public class AiWaveConcatenatedSrcWavList
{
    public string WavFilePath = null!;
    public string? FilterName;
    [JsonConverter(typeof(StringEnumConverter))]
    public AiAudioEffectSpeedType? FilterSpeedType;
    public JObject? FilterSettings;
    public int TargetStartMsec;
    public int TargetEndMsec;
    public int SourceStartMsec;
    public int SourceEndMsec;
}


public static class AiWaveConcatenateWithCrossFadeUtil
{
    /// <summary>
    /// srcWavFilesQueue に含まれる WAV ファイルを順次読み込み、
    /// 部分的に切り取ってフェードイン・フェードアウトを施し、
    /// クロスフェードしながら合成した WAV ファイルを作成する。
    /// </summary>
    /// <param name="srcWaveFilesList">ソースの wav ファイル パスを格納したキュー</param>
    /// <param name="singleFilePartMSecs">切り出し目安の長さ（ミリ秒）</param>
    /// <param name="totalDurationMsecs">出力 WAV の合計長さ（ミリ秒）</param>
    /// <param name="fadeoutMsecs">フェードイン／フェードアウトの時間（ミリ秒）</param>
    /// <param name="dstWavFilePath">出力先 wav ファイル パス</param>
    /// <param name="cancel">キャンセル用トークン</param>
    /// <returns></returns>
    public static async Task<List<AiWaveConcatenatedSrcWavList>> ConcatenateAsync(
        ShuffledEndlessQueue<string> srcWavFilesQueue,
        int totalDurationMsecs,
        AiRandomBgmSettings settings,
        string dstWavFilePath,
        CancellationToken cancel = default)
    {
        if (srcWavFilesQueue.Count == 0)
        {
            throw new ArgumentException("ソースの WAV ファイル キューが空です。");
        }
        if (settings.Medley_SingleFilePartMSecs <= 0 ||
            totalDurationMsecs <= 0 ||
            settings.Medley_FadeInOutMsecs <= 0)
        {
            throw new ArgumentException("パラメータが不正です。");
        }

        List<AiWaveConcatenatedSrcWavList> retSrcList = new List<AiWaveConcatenatedSrcWavList>();

        //------------------------------------------------------------
        // まずは1つ目の WAV ファイルからフォーマット情報を取得する
        // キューをいったん取り出し、また戻す
        //------------------------------------------------------------
        string firstWav = srcWavFilesQueue.Peek();
        WaveFormat? waveFormat;
        await using (var firstFs = new FileStream(firstWav, FileMode.Open, FileAccess.Read, FileShare.Read, 4096, useAsync: true))
        using (var firstReader = new WaveFileReader(firstFs))
        {
            waveFormat = firstReader.WaveFormat;
        }

        // 以降の処理では、全ファイルが同じ waveFormat である前提

        //------------------------------------------------------------
        // 必要な変数を用意
        //------------------------------------------------------------
        var random = new Random(Secure.RandSInt31());
        int currentTotalLengthMsecs = 0;
        var partialDataList = new List<float[]>();  // 各「部分音声データ」（すでにフェードイン・アウトが施されているもの）を格納

        // fade 時のサンプル数
        int fadeSamples = (int)((long)settings.Medley_FadeInOutMsecs * waveFormat.SampleRate / 1000) * waveFormat.Channels;

        //------------------------------------------------------------
        // ソース WAV のキューを消費しながら部分的に読み取る
        //------------------------------------------------------------
        while (srcWavFilesQueue.Count > 0)
        {
            cancel.ThrowIfCancellationRequested();

            string wavPath = srcWavFilesQueue.Dequeue();

            // WAV の総再生時間などを取得
            // （NAudio は完全な async 読み込みではないが、FileStream を useAsync: true で開く）
            int fileTotalMs;
            await using (var fs = new FileStream(wavPath, FileMode.Open, FileAccess.Read, FileShare.Read, 4096))
            using (var reader = new WaveFileReader(fs))
            {
                fileTotalMs = (int)reader.TotalTime.TotalMilliseconds;

                // ファイルが 2*fadeoutMsecs 未満ならスキップ
                if (fileTotalMs < (settings.Medley_FadeInOutMsecs * 2))
                {
                    continue;
                }

                // singleFilePartMSecs に対して上下30% の乱数を加味して thisFilePartMSecs を求める
                //int variation = (int)((double)singleFilePartMSecs * 0.3);
                int thisFilePartMSecs = Util.GenRandInterval(settings.Medley_SingleFilePartMSecs);
                if (thisFilePartMSecs < 0) thisFilePartMSecs = 0;

                // もし thisFilePartMSecs > fileTotalMs ならクリップ
                if (thisFilePartMSecs > fileTotalMs)
                {
                    thisFilePartMSecs = fileTotalMs;
                }

                if (fileTotalMs < (thisFilePartMSecs + settings.Medley_MarginMsecs * 2))
                {
                    continue;
                }

                // ランダム開始位置 (0 ～ (fileTotalMs - thisFilePartMSecs))
                int maxStartMs = fileTotalMs - thisFilePartMSecs;
                if (maxStartMs < 0) maxStartMs = 0; // 念のため

                int randStartMs = settings.Medley_MarginMsecs;
                int randEndMs = maxStartMs - settings.Medley_MarginMsecs;
                if (randEndMs < 0) randEndMs = 0; // 念のため

                int startMs = random.Next(randStartMs, randEndMs + 1);

                // 必要サンプル数の計算
                int readSamples = MsecToSamples(thisFilePartMSecs, waveFormat);
                int startOffsetSamples = MsecToSamples(startMs, waveFormat);

                // WaveFileReader をサンプルプロバイダ化して読み取る
                // まず、開始位置までシーク
                reader.CurrentTime = TimeSpan.FromMilliseconds(startMs);
                var sampleProvider = reader.ToSampleProvider();  // 32-bit float 変換プロバイダ

                // 部分波形を取得
                float[] partBuffer = new float[readSamples];
                int actuallyRead = 0;
                int totalRead = 0;
                do
                {
                    cancel.ThrowIfCancellationRequested();

                    // sampleProvider.Read は同期的に読み取るが、ファイルストリーム自体は async で開いている
                    actuallyRead = sampleProvider.Read(partBuffer, totalRead, readSamples - totalRead);
                    totalRead += actuallyRead;
                } while (actuallyRead > 0 && totalRead < readSamples);

                if (totalRead < readSamples)
                {
                    // 端数しか読めなかった場合、足りない分は 0 で埋める
                    for (int i = totalRead; i < readSamples; i++)
                    {
                        partBuffer[i] = 0f;
                    }
                }

                AiWaveConcatenatedSrcWavList item = new AiWaveConcatenatedSrcWavList { WavFilePath = wavPath };

                item.SourceStartMsec = startMs;
                item.SourceEndMsec = thisFilePartMSecs;

                if (settings.CreateAudioEffectFilterForNormalMusicPartProc != null)
                {
                    var filter = settings.CreateAudioEffectFilterForNormalMusicPartProc(cancel);
                    if (filter != null)
                    {
                        // オーディオフィルタ関数を適用。ただし、float の波形データは直接扱えないので、いったん byte[] に変換し、フィルタを通し、また float[] に戻す処理が必要
                        var partWaveData = AiWaveUtil.ConvertFloatArrayToPcm16Memory(partBuffer, cancel);

                        filter.PerformFilterFunc(partWaveData, cancel);

                        partBuffer = AiWaveUtil.ConvertPcm16ToFloatArray(partWaveData, cancel);

                        item.FilterName = filter.FilterName;
                        item.FilterSettings = filter.EffectSettings._ToJObject();
                        item.FilterSpeedType = filter.FilterSpeedType;
                    }
                }

                item.TargetStartMsec = currentTotalLengthMsecs;
                item.TargetEndMsec = currentTotalLengthMsecs + thisFilePartMSecs - settings.Medley_FadeInOutMsecs;

                retSrcList.Add(item);

                // フェードイン／フェードアウトを適用
                ApplyFadeInOut(partBuffer, fadeSamples);

                // この部分波形をリストに追加
                partialDataList.Add(partBuffer);

                // 加算
                currentTotalLengthMsecs += (thisFilePartMSecs - settings.Medley_FadeInOutMsecs);
            }

            // もし合計長が totalDurationMsecs を超えたらループを抜ける
            if (currentTotalLengthMsecs >= (totalDurationMsecs + settings.Medley_FadeInOutMsecs * 2))
            {
                break;
            }
        }

        //------------------------------------------------------------
        // partialDataList に「部分音声データ」が集まったので、
        // これをクロスフェードしながら最終波形を作ってファイルに書き込む。
        //------------------------------------------------------------
        int totalDurationSamples = MsecToSamples(totalDurationMsecs, waveFormat);

        // 最終出力用バッファ（float で一度合成）
        float[] finalMixBuffer = new float[totalDurationSamples];
        // ゼロ埋めされていると仮定

        // クロスフェード重ね合わせ用
        // ある部分音声データ i と i+1 のフェードアウト／フェードイン部を overlap する。
        int overlapSamples = fadeSamples; // fadeoutMsecs に相当するサンプル数

        // 現在の書き込み先位置（サンプル単位）
        int currentPos = 0;

        for (int i = 0; i < partialDataList.Count; i++)
        {
            cancel.ThrowIfCancellationRequested();

            float[] partData = partialDataList[i];
            int partLen = partData.Length;

            if (i == 0)
            {
                // 最初の部分音声データはそのまま貼り付け開始
                WriteOverlapAdd(
                    source: partData,
                    sourceOffset: 0,
                    sourceCount: partLen,
                    dest: finalMixBuffer,
                    destOffset: 0);

                currentPos = partLen;  // 貼り付け終わりの次のサンプル位置
            }
            else
            {
                // 2 個目以降は overlapSamples だけ前に重ね合わせる
                int startPos = currentPos - overlapSamples;
                if (startPos < 0) startPos = 0;

                // 重ね合わせる分（partData の先頭 overlapSamples と、finalMix の後ろ overlapSamples）
                int overlapCount = overlapSamples;
                if (overlapCount > partLen) overlapCount = partLen;

                // オーバーラップ部分
                WriteOverlapAdd(
                    source: partData,
                    sourceOffset: 0,
                    sourceCount: overlapCount,
                    dest: finalMixBuffer,
                    destOffset: startPos);

                // オーバーラップ後の残り部分をコピー
                int remain = partLen - overlapCount;
                if (remain > 0)
                {
                    int writeDestPos = startPos + overlapCount;
                    // finalMixBuffer が残り書ける長さ
                    int canWrite = finalMixBuffer.Length - writeDestPos;
                    if (canWrite < 0) canWrite = 0;
                    if (remain > canWrite)
                    {
                        remain = canWrite;
                    }

                    if (remain > 0)
                    {
                        WriteOverlapAdd(
                            source: partData,
                            sourceOffset: overlapCount,
                            sourceCount: remain,
                            dest: finalMixBuffer,
                            destOffset: writeDestPos);
                    }
                }

                // 次の書き込み先位置は「(前回の終端) + (今回パート長) - overlap」
                currentPos += (partLen - overlapSamples);
            }

            // もし finalMixBuffer の長さを超える場合、そこで打ち切る
            if (currentPos >= totalDurationSamples)
            {
                break;
            }
        }

        // もしまだ totalDurationSamples より短ければ、残りは無音(0)ですでに埋まっているので OK

        //------------------------------------------------------------
        // finalMixBuffer を waveFormat のフォーマット(例: 16bit PCM) でファイルに書き込む
        //------------------------------------------------------------
        await using (var outFs = new FileStream(dstWavFilePath, FileMode.Create, FileAccess.Write, FileShare.None, 4096, useAsync: true))
        using (var writer = new WaveFileWriter(outFs, waveFormat))
        {
            // フォーマットが 16bit PCM の場合を想定した例。
            // （もしオリジナルが IEEE Float だった場合はそのまま float -> バイト列変換で書き込んでも良いです。
            //   あるいは NAudio の SampleProvider 系を駆使して WaveFileWriter に書くアプローチも可能です。）
            if (waveFormat.Encoding == WaveFormatEncoding.Pcm && waveFormat.BitsPerSample == 16)
            {
                // float => short に変換しながら書き込み
                short[] shortBuffer = new short[finalMixBuffer.Length];
                for (int i = 0; i < finalMixBuffer.Length; i++)
                {
                    // float(-1.0～1.0) -> short(-32768～32767)
                    float val = finalMixBuffer[i];
                    if (val > 1.0f) val = 1.0f;
                    if (val < -1.0f) val = -1.0f;
                    shortBuffer[i] = (short)(val * short.MaxValue);
                }

                // バイナリデータとして書く
                // WriteSamples は 16-bit の場合、配列長の 2倍バイト数が書き込まれる
                writer.WriteSamples(shortBuffer, 0, shortBuffer.Length);
            }
            else
            {
                // PCM 16bit 以外の場合、適宜実装を変更
                // 例: 32-bit float の WaveFormat を作って書く、など。
                throw new NotImplementedException("16bit PCM 以外の形式は未対応のサンプルです。");
            }
        }

        // 完了
        return retSrcList;
    }

    /// <summary>
    /// ミリ秒をサンプル数に変換するヘルパー関数
    /// </summary>
    private static int MsecToSamples(int msec, WaveFormat format)
    {
        long samples = (long)format.SampleRate * msec / 1000 * format.Channels;
        if (samples > int.MaxValue) return int.MaxValue;
        return (int)samples;
    }

    /// <summary>
    /// 部分波形データにフェードイン／フェードアウトを掛ける（In/Out の長さは同じ）
    /// </summary>
    /// <param name="data">処理対象データ（float配列、-1～1 程度を想定）</param>
    /// <param name="fadeSamples">フェードに要するサンプル数</param>
    private static void ApplyFadeInOut(float[] data, int fadeSamples)
    {
        if (data.Length < fadeSamples * 2) return; // 既にこの時点で短すぎる場合は何もしない

        // フェードイン
        for (int i = 0; i < fadeSamples; i++)
        {
            float mul = (float)i / fadeSamples; // 0 ～ 1
            data[i] *= mul;
        }
        // フェードアウト
        for (int i = 0; i < fadeSamples; i++)
        {
            int idx = data.Length - fadeSamples + i;
            float mul = 1.0f - (float)i / fadeSamples; // 1 ～ 0
            data[idx] *= mul;
        }
    }

    /// <summary>
    /// source を dest に加算書き込み（OverlapAdd）するヘルパー
    /// </summary>
    private static void WriteOverlapAdd(
        float[] source,
        int sourceOffset,
        int sourceCount,
        float[] dest,
        int destOffset)
    {
        int sMax = sourceOffset + sourceCount;
        int dMax = destOffset + sourceCount;
        if (sMax > source.Length) sMax = source.Length;
        if (dMax > dest.Length) dMax = dest.Length;

        int length = Math.Min(sMax - sourceOffset, dMax - destOffset);
        for (int i = 0; i < length; i++)
        {
            dest[destOffset + i] += source[sourceOffset + i];
        }
    }
}

public class ProcessAudioEffect1Setting
{
    /// <summary>
    /// ステレオ揺れを生むLFOの基本周波数(Hz)
    /// 例: 0.25 ならば約4秒周期の揺れ
    /// </summary>
    public double LfoFrequency = 0.25;

    /// <summary>
    /// 揺れの深さ(0.0～1.0程度を推奨)
    /// 1.0 に近いほど左右の振り幅が大きくなる
    /// </summary>
    public double Depth = 0.5;

    /// <summary>
    /// 不規則なランダム揺らぎを混ぜる比率(0.0～1.0)
    /// 0.0で完全に規則的(純粋なLFO)、1.0で強いランダム揺らぎ
    /// </summary>
    public double RandomFactor = 0.3;

    /// <summary>
    /// LFOの波形の位相進行スピードに掛ける倍率
    /// (LfoFrequency が小さくても、この値を大きくすれば速く変調される)
    /// </summary>
    public double LfoSpeedMultiplier = 1.0;

    /// <summary>
    /// 不規則変動(Random Walk)のステップサイズ
    /// 数値を大きくすると、より激しく不規則に変動する
    /// </summary>
    public double RandomStepSize = 0.0005;

    /// <summary>
    /// 不規則変動を制限する(振幅が大きくなりすぎない)ための最大値
    /// </summary>
    public double RandomWalkLimit = 1.0;
}

public class ProcessAudioEffect2Setting
{
    /// <summary>
    /// LFO等でボリュームを変動させる際の振幅 (0.0～1.0 程度)
    /// </summary>
    public double Depth { get; set; } = 0.5;

    /// <summary>
    /// ゆらぎの最小周波数(Hz)。UseRandomFrequency が true の場合、これと FrequencyMax の範囲内でランダムに変化する
    /// </summary>
    public double FrequencyMin { get; set; } = 0.2;

    /// <summary>
    /// ゆらぎの最大周波数(Hz)。UseRandomFrequency が true の場合、これと FrequencyMin の範囲内でランダムに変化する
    /// </summary>
    public double FrequencyMax { get; set; } = 1.0;

    /// <summary>
    /// 周波数をリアルタイムにランダム変化させるかどうか
    /// </summary>
    public bool UseRandomFrequency { get; set; } = true;

    /// <summary>
    /// ボリュームゆらぎ以外に位相差(微小ディレイ)による効果を与えるかどうか
    /// </summary>
    public bool EnablePhaseShift { get; set; } = true;

    /// <summary>
    /// 位相差(サンプル数)。EnablePhaseShift=true のとき適用
    /// 例えば 10 サンプル程度のディレイを片側チャネルにだけ付加し、頭の上下や左右に動く錯覚をより強調する
    /// </summary>
    public int PhaseShiftSamples { get; set; } = 10;

    /// <summary>
    /// 効果の種類を表すサンプル enum
    /// </summary>
    public EffectType EffectMode { get; set; } = EffectType.Basic;

    [Flags]
    public enum EffectType
    {
        Basic,
        Extended,
        Crazy
    }
}



public class ProcessAudioEffect3Setting
{
    /// <summary>
    /// 1サイクル(遠方→接近→通過→離脱→遠方に戻る)に要する時間(秒)。
    /// 例: 2.0 なら2秒で1往復する。
    /// </summary>
    public double CycleSeconds = 2.0;

    /// <summary>
    /// 最遠時の音量(距離感の演出)。0.0～1.0程度で設定。
    /// 例: 0.2 くらいにすると、最遠時は音が小さくなる。
    /// </summary>
    public double VolumeFar = 0.2;

    /// <summary>
    /// 最接近時の音量(距離感の演出)。0.0～1.0程度で設定。
    /// 例: 1.0 なら最接近時は原音量(最大)になる。
    /// </summary>
    public double VolumeNear = 1.0;

    /// <summary>
    /// パン幅。-1.0(完全左)～+1.0(完全右)を振り切るなら 1.0 等。
    /// 数値を小さくすると、左右の振れ幅が抑えられる。
    /// </summary>
    public double PanWidth = 1.0;

    /// <summary>
    /// パン方向を反転するかのフラグ。true なら左右を反転して演出。
    /// </summary>
    public bool ReversePan = false;

    /// <summary>
    /// ドップラー効果のようなピッチ変化をシミュレートする場合の強度。
    /// 0.0 ならピッチ変化なし。1.0 ならサイクルに応じたある程度のピッチ変化。
    /// 大きくしすぎると不自然な音程になる場合がある。
    /// </summary>
    public double DopplerFactor = 0.0;

    // 必要に応じて他のパラメータ(フランジャーの深さやLFOの形状など)を追加してもよい。
}

public static class AudioEffectProcessor
{
    /// <summary>
    /// 44.1kHz/16bit/ステレオ の WAV PCM データ(ヘッダ抜き)をメモリ上で加工する。
    /// targetWav 内のデータを直接書き換える。
    /// </summary>
    /// <param name="targetWav">加工対象の WAV PCM データ(ヘッダなし、44.1kHz/16bit/stereo)</param>
    /// <param name="setting">効果パラメータ</param>
    public static void ProcessAudioEffect3(Memory<byte> targetWav, ProcessAudioEffect3Setting setting)
    {
        // 16bitステレオなので、
        //  - 1サンプル = 左チャネル(short) + 右チャネル(short) = 4バイト
        // 全サンプル数 = targetWav.Length / 4

        const int sampleRate = 44100;
        int totalBytes = targetWav.Length;
        int totalSamples = totalBytes / 2;     // 16bitごと(=2バイト単位)の数
        int totalFrames = totalSamples / 2;   // ステレオ(Left+Right)で1フレームなので /2

        // スパンを取得
        var span = targetWav.Span;

        // ドップラー効果(ピッチ変化)を行う場合のために、読み出し位置(実サンプル位置)と書き込み位置を別管理
        // ただし、ここでは簡易的に同じバッファに上書きするので、実際には大掛かりなバッファの再配置が必要になる場合がある。
        // 今回のデモではあまり厳密にはやらず、単純に「読み書き同じ位置だが、一応サイクルに合わせてピッチを少し揺らす」イメージ。
        double dopplerFactor = setting.DopplerFactor;
        double pitchPos = 0.0;   // ドップラー時の「サンプル再生位置」の仮想値
        double pitchStep = 1.0;  // 1サンプルごとの前進量(=1.0が等速、>1.0で再生が早くなりピッチ↑、<1.0で遅くなりピッチ↓)

        for (int frameIndex = 0; frameIndex < totalFrames; frameIndex++)
        {
            // 時間(t)を計算 (単位: 秒)
            // ドップラーでサンプル読み出し位置がずれる場合は本来 pitchPos / sampleRate などだが、
            // ここでは frameIndex を使った「実処理中のサンプル時刻」として計算(簡易版)。
            double t = (double)frameIndex / sampleRate;

            // 1サイクルあたり CycleSeconds なので、その位相を 0～1 で算出
            double phase = (t % setting.CycleSeconds) / setting.CycleSeconds;

            // 距離感(音量)の変化: ここでは 0～π のコサイン波を使って [VolumeFar..VolumeNear] を往復させるイメージ
            // (好みに応じてサイン波でも三角波でも可)
            // 例: コサイン波で 1→-1 に変化させ、そこから音量をマッピング
            double cosVal = Math.Cos(phase * Math.PI * 2.0); // -1.0 ～ +1.0
            // -1.0～+1.0 を 0.0～1.0 に変換 => (cosVal + 1.0)/2.0
            double distLerp = (cosVal + 1.0) / 2.0; // 0～1
            // VolumeFar～VolumeNear に線形補間
            double volume = setting.VolumeFar + (setting.VolumeNear - setting.VolumeFar) * distLerp;

            // 左右パン(サイン波で -1.0～+1.0 に振る)
            double panSign = setting.ReversePan ? -1.0 : 1.0;
            double panVal = Math.Sin(phase * Math.PI * 2.0) * panSign; // -1.0 ～ +1.0
            // 左チャンネルと右チャンネルのそれぞれに掛ける係数を決定
            // 通常は pan=-1.0 で L=1.0,R=0.0、pan=+1.0 で L=0.0,R=1.0 のように計算するが、
            // ここでは stereo balance の簡易式として採用
            double leftFactor = (1.0 - panVal) * 0.5;   // panVal=-1 => leftFactor=1.0, panVal=+1 => leftFactor=0.0
            double rightFactor = (1.0 + panVal) * 0.5;  // panVal=-1 => rightFactor=0.0, panVal=+1 => rightFactor=1.0

            // PanWidth を加味(±PanWidthの範囲に縮める・広げる)
            // 例: PanWidth=1.0 => -1～+1 をそのまま, PanWidth=0.5 => -0.5～+0.5 に
            //   => その結果、leftFactor, rightFactor も中央寄りになる
            double effectivePanVal = panVal * setting.PanWidth;
            leftFactor = (1.0 - effectivePanVal) * 0.5;
            rightFactor = (1.0 + effectivePanVal) * 0.5;

            // 実際の読み書き位置を求める
            // 簡易的に frameIndex をそのまま使うが、ドップラーFactor を考慮してピッチステップを変動させる例
            // (あまり精密ではないが効果の雰囲気を出すためのデモ)
            double dopplerPhase = (phase * 2.0 - 1.0); // -1.0～+1.0
            // 正接近時(phase~0.5あたり)にピッチが上がり、離れる時にピッチが下がるようなイメージ
            // (好きな数式にしてOK)
            pitchStep = 1.0 + dopplerFactor * dopplerPhase * 0.5;
            pitchPos += pitchStep; // 次回に向けて進める

            // ピッチ変化を適用した「読み出しサンプル位置」
            int sampleReadIndex = (int)pitchPos;

            // バッファの外を読まないようにクリップ
            if (sampleReadIndex < 0) sampleReadIndex = 0;
            if (sampleReadIndex >= totalFrames) sampleReadIndex = totalFrames - 1;

            // 実際の書き込み先 frameIndex に対し、読み出し元は sampleReadIndex を参照
            // WAV PCM データは [L, R, L, R, ...] の順で short (2バイト) が交互に並んでいる
            int readOffset = sampleReadIndex * 2 * sizeof(short);
            short sampleL = MemoryMarshal.Read<short>(span.Slice(readOffset, 2));
            short sampleR = MemoryMarshal.Read<short>(span.Slice(readOffset + 2, 2));

            // volume (距離感) とパン係数 を掛け合わせて出力サンプルを計算
            double outL = sampleL * volume * leftFactor;
            double outR = sampleR * volume * rightFactor;

            // 16bitの範囲にクリップ(-32768～32767)
            short finalL = (short)Math.Clamp((int)outL, short.MinValue, short.MaxValue);
            short finalR = (short)Math.Clamp((int)outR, short.MinValue, short.MaxValue);

            // 書き込み先
            int writeOffset = frameIndex * 2 * sizeof(short);
            MemoryMarshal.Write(span.Slice(writeOffset, 2), ref finalL);
            MemoryMarshal.Write(span.Slice(writeOffset + 2, 2), ref finalR);
        }
    }

    /// <summary>
    /// targetWav(44.1kHz/16bit/ステレオ)のPCMデータに対して
    /// 「ゆらゆら動くような幻想的効果」を与えるサンプル処理
    /// </summary>
    /// <param name="targetWav">
    /// 加工対象の WAVファイル音声データ (ヘッダ無し)。44.1kHz/2ch/16bit PCM
    /// </param>
    /// <param name="setting">
    /// 効果設定パラメータ
    /// </param>
    public static void ProcessAudioEffect2(Memory<byte> targetWav, ProcessAudioEffect2Setting setting)
    {
        // 44.1kHz, 2ch, 16bit => 1サンプル(フレーム)あたり4バイト
        const int BytesPerSample = 2;   // 16bit
        const int Channels = 2;        // ステレオ
        const int BytesPerFrame = BytesPerSample * Channels;
        const int SampleRate = 44100;

        // PCMデータ全体のフレーム数(ステレオフレーム数)
        int totalFrames = targetWav.Length / BytesPerFrame;
        if (totalFrames == 0) return;

        // メモリから配列を取り出す (Span<>を利用するサンプル)
        Span<byte> dataSpan = targetWav.Span;

        // -- パラメータを取得 --
        double depth = setting.Depth;               // ボリューム変動の振幅 (0.0～1.0想定)
        double freqMin = Math.Max(0.0, setting.FrequencyMin);
        double freqMax = Math.Max(freqMin, setting.FrequencyMax);
        bool useRandomFreq = setting.UseRandomFrequency;
        bool enablePhaseShift = setting.EnablePhaseShift;
        int phaseShiftSamples = Math.Max(0, setting.PhaseShiftSamples);

        // LFO用の状態管理変数 (ランダム周波数など)
        Random rand = new Random();
        double currentFreq = freqMin + (freqMax - freqMin) * rand.NextDouble(); // 初期周波数
        double phase = 0.0;   // LFOの位相(0～2πを想定してループさせる)
        double twoPi = 2.0 * Math.PI;

        // 周期的に周波数を変化させる (サンプル実装として数千フレームおきに変化)
        int framesUntilNextFreqChange = 2000;

        // 片チャネルへのディレイ用に、ディレイバッファを準備（Rightチャネルを遅らせる例）
        // ディレイバッファ = phaseShiftSamples 個のステレオフレーム分を取る
        // ただし Right チャンネルだけずらすので、右チャネル分だけあればよい
        short[] delayBuffer = new short[phaseShiftSamples];

        // EffectMode によって処理を変えても良い (ここでは簡単にswitchだけ用意)
        switch (setting.EffectMode)
        {
            case ProcessAudioEffect2Setting.EffectType.Basic:
                // 何もしない特別ケース or 下記の標準処理と同一にする
                break;
            case ProcessAudioEffect2Setting.EffectType.Extended:
                // もう少し激しく LFO をかける等
                depth *= 1.2;
                break;
            case ProcessAudioEffect2Setting.EffectType.Crazy:
                // さらに大きく、かつ位相ディレイも絶対ONにしてしまう等
                depth *= 1.5;
                enablePhaseShift = true;
                break;
        }

        // -- メインループ --
        for (int i = 0; i < totalFrames; i++)
        {
            // ランダム周波数変化
            if (useRandomFreq)
            {
                if (--framesUntilNextFreqChange <= 0)
                {
                    // 周波数を (freqMin ～ freqMax) の範囲で新しくランダムに決める
                    currentFreq = freqMin + (freqMax - freqMin) * rand.NextDouble();
                    framesUntilNextFreqChange = 2000 + rand.Next(2000); // 次の変化までをまた適当に決める
                }
            }

            // 現在の LFO位相に基づいてボリューム係数を計算する ( -1.0 ～ +1.0 )
            double lfo = Math.Sin(phase);

            // この lfo を 0.0～1.0 にマッピングして左右に振るようなイメージ
            // 例：Left = 1 - (depth * lfo), Right = 1 + (depth * lfo)
            // ただし -1.0～+1.0 の範囲 → depth分だけ縮めて左右を変調
            double leftGain = 1.0 - (depth * lfo);
            double rightGain = 1.0 + (depth * lfo);

            // 16bitステレオフレームの先頭バイト位置
            int frameIndex = i * BytesPerFrame;

            // 左チャネルサンプル (16bit = 2byte, little endian)
            short leftSample = (short)((dataSpan[frameIndex + 1] << 8) | dataSpan[frameIndex]);

            // 右チャネルサンプル
            short rightSample = (short)((dataSpan[frameIndex + 3] << 8) | dataSpan[frameIndex + 2]);

            // 位相シフトをかける場合、まず右チャネルの過去サンプルを使う
            short delayedRightSample = rightSample;
            if (enablePhaseShift && phaseShiftSamples > 0)
            {
                // delayBuffer[0]に最も古いサンプル、末尾に最も新しいサンプルを突っ込むイメージ
                // まず取り出し
                delayedRightSample = delayBuffer[0];

                // シフト
                Buffer.BlockCopy(delayBuffer, sizeof(short), delayBuffer, 0, (phaseShiftSamples - 1) * sizeof(short));
                // 新しいサンプルを末尾へ
                delayBuffer[phaseShiftSamples - 1] = rightSample;
            }

            // LFOボリュームを適用
            double newLeft = leftSample * leftGain;
            double newRight = delayedRightSample * rightGain;

            // オーバーフローしないように short 範囲(-32768～32767)へクリップ
            short outLeft = (short)Math.Max(short.MinValue, Math.Min(short.MaxValue, newLeft));
            short outRight = (short)Math.Max(short.MinValue, Math.Min(short.MaxValue, newRight));

            // メモリに書き戻し (little endian)
            dataSpan[frameIndex] = (byte)(outLeft & 0xFF);
            dataSpan[frameIndex + 1] = (byte)((outLeft >> 8) & 0xFF);
            dataSpan[frameIndex + 2] = (byte)(outRight & 0xFF);
            dataSpan[frameIndex + 3] = (byte)((outRight >> 8) & 0xFF);

            // 位相を進める (LFOの 1サンプル刻み)
            // currentFreq [Hz] -> 1秒間に currentFreq [周期] 進行 -> 1サンプル (1/44100秒) で位相は 2π * (currentFreq / 44100)
            phase += (twoPi * currentFreq / SampleRate);
            // 位相が 2π を超えたら折り返し
            if (phase > twoPi) phase -= twoPi;
        }
    }

    /// <summary>
    /// ステレオPCM(44.1kHz/16bit/2ch, WAVヘッダなし)のデータを不規則なステレオ回転効果で加工する。
    /// </summary>
    /// <param name="targetWav">対象のPCMデータ(ステレオ, 16bit, 44.1kHz)</param>
    /// <param name="setting">効果パラメータ</param>
    public static void ProcessAudioEffect1(Memory<byte> targetWav, ProcessAudioEffect1Setting setting)
    {
        // サンプルレート、チャンネル数、ビット深度は固定
        const int sampleRate = 44100;
        const int channels = 2;
        const int bytesPerSample = 2; // 16bit = 2bytes
        int frameSize = channels * bytesPerSample; // 1フレーム(左右2ch分)あたり4バイト

        // targetWav のサイズから総フレーム数を求める
        int totalFrames = targetWav.Length / frameSize;
        if (totalFrames == 0) return; // データが不十分なら何もしない

        // スパンを取得（in-place 処理）
        Span<byte> dataSpan = targetWav.Span;

        // LFO用の位相、RandomWalk用の現在値
        double phase = 0.0;
        double randomWalkValue = 0.0;

        // ランダム生成器
        Random rand = new Random();

        // 1サンプルあたりの位相進行量(2π * 周波数 / サンプルレート)に速度倍率を掛ける
        double phaseIncrement = 2.0 * Math.PI * setting.LfoFrequency / sampleRate * setting.LfoSpeedMultiplier;

        for (int i = 0; i < totalFrames; i++)
        {
            // 現在のLFO値( -1.0 ～ +1.0 )
            double lfo = Math.Sin(phase);

            // ランダム揺らぎの計算: ランダムウォークで -step ～ +step 変動
            double step = (rand.NextDouble() - 0.5) * 2.0 * setting.RandomStepSize;
            randomWalkValue += step;
            // 振幅制限(±RandomWalkLimit)
            if (randomWalkValue > setting.RandomWalkLimit) randomWalkValue = setting.RandomWalkLimit;
            if (randomWalkValue < -setting.RandomWalkLimit) randomWalkValue = -setting.RandomWalkLimit;

            // LFO値とランダムウォーク値を、RandomFactor の比率でミックス
            // RandomFactor=0なら純粋LFO、1なら純粋RandomWalk
            double mixedValue = (1.0 - setting.RandomFactor) * lfo
                              + setting.RandomFactor * randomWalkValue;

            // 混合値にDepthを掛ける( -Depth ～ +Depth )
            double pan = setting.Depth * mixedValue;

            // 左右ゲイン計算
            // pan>0 のとき Right が大きく、pan<0 のとき Left が大きくなるイメージ
            // ここでは「中央1.0 ± pan」で単純に左右をシフト
            double leftGain = 1.0 - pan;
            double rightGain = 1.0 + pan;

            // 左右チャンネルのサンプルを取り出して、ゲインを掛けて書き戻す
            int baseIndex = i * frameSize;

            // 左サンプル(16bit)取得
            short leftSample = (short)(dataSpan[baseIndex] | (dataSpan[baseIndex + 1] << 8));
            // 右サンプル(16bit)取得
            short rightSample = (short)(dataSpan[baseIndex + 2] | (dataSpan[baseIndex + 3] << 8));

            // 倍率適用(オーバーフローを避けるためにintで計算してからClamp)
            int newLeft = (int)Math.Round(leftSample * leftGain);
            int newRight = (int)Math.Round(rightSample * rightGain);

            // 16bitの範囲にクリップ(-32768～32767)
            newLeft = Math.Clamp(newLeft, short.MinValue, short.MaxValue);
            newRight = Math.Clamp(newRight, short.MinValue, short.MaxValue);

            // 書き戻し(リトルエンディアン)
            dataSpan[baseIndex + 0] = (byte)(newLeft & 0xFF);
            dataSpan[baseIndex + 1] = (byte)((newLeft >> 8) & 0xFF);
            dataSpan[baseIndex + 2] = (byte)(newRight & 0xFF);
            dataSpan[baseIndex + 3] = (byte)((newRight >> 8) & 0xFF);

            // 位相を進める
            phase += phaseIncrement;
            if (phase > 2.0 * Math.PI)
            {
                phase -= 2.0 * Math.PI; // 周期を越えたら折り返し
            }
        }
    }
}



public static class AiWaveVolumeUtils
{
    /// <summary>
    /// 1フレーム(ステレオ2サンプル)当たりの無音とみなす振幅の目安 (dB)
    /// </summary>
    private const double GateThresholdDb = -50.0; // -50 dB
    /// <summary>
    /// 線形振幅に変換した際のしきい値 (± 振幅)
    /// </summary>
    private static readonly double GateThresholdAmplitude = Math.Pow(10.0, GateThresholdDb / 20.0); // 約 0.00316

    /// <summary>
    /// (関数1) WAV のステレオ波形データ(16ビット, L/R)から、ゲート付きの平均ボリューム(dB)を返す。
    /// </summary>
    /// <param name="waveFileIn">
    ///     ステレオ WAV(44.1 kHz, 16bit, 2ch)の生 PCM データ(ヘッダ抜き)
    ///     [L, R, L, R, ...] の順番で交互に 16 ビット値が並ぶ。
    /// </param>
    /// <param name="cancel">中断用トークン</param>
    /// <returns>左右チャンネルを合成した RMS の平均値(ゲート除外あり)を dB 表記した値</returns>
    public static double CalcMeanVolume(Memory<byte> waveFileIn, CancellationToken cancel)
    {
        // 1サンプル(16bit) = 2バイト
        // ステレオなので1フレーム = 4バイト (L 2byte + R 2byte)
        // 全サンプル数(左右合計) = waveFileIn.Length / 2
        // フレーム数(ステレオ)  = waveFileIn.Length / 4
        int frameCount = waveFileIn.Length / 4;
        if (frameCount == 0)
        {
            // PCM データが無い場合は -∞ に近い値として返す (例: -90 dB)
            return -90.0;
        }

        var span = waveFileIn.Span;

        double sumOfSquares = 0.0;  // RMS 用(振幅^2 の合計)
        long validCount = 0;        // ゲートしきい値を超えたフレーム数

        for (int i = 0; i < frameCount; i++)
        {
            cancel.ThrowIfCancellationRequested();

            // ステレオ L/R の 16bit サンプル値を取得 (リトルエンディアン)
            short left = (short)((span[4 * i + 1] << 8) | span[4 * i + 0]);
            short right = (short)((span[4 * i + 3] << 8) | span[4 * i + 2]);

            // [-32768, 32767] → [-1.0, +1.0] へ正規化
            double leftFloat = left / 32768.0;
            double rightFloat = right / 32768.0;

            // ステレオのフレームRMS: sqrt( (L^2 + R^2) / 2 ) 
            double frameAmplitude = Math.Sqrt((leftFloat * leftFloat + rightFloat * rightFloat) / 2.0);

            // ゲートを超えるか判定 (-50 dB 相当)
            if (frameAmplitude > GateThresholdAmplitude)
            {
                // フレームRMS を二乗したものを積算 (RMS計算用)
                sumOfSquares += (frameAmplitude * frameAmplitude);
                validCount++;
            }
        }

        if (validCount == 0)
        {
            // 全てがほぼ無音ゲート以下の場合
            return -90.0; // 仮に -90 dB とする
        }

        // 有効フレームの平均平方値
        double meanSquare = sumOfSquares / validCount;
        // RMS に戻す
        double rms = Math.Sqrt(meanSquare);

        // dB化 (20 * log10(rms))
        // rms が非常に小さい場合は -∞ に行かないようクリップ
        if (rms < 1.0e-10)
        {
            return -90.0;
        }

        double volumeDb = 20.0 * Math.Log10(rms);
        return volumeDb;
    }

    /// <summary>
    /// (関数2) 指定された平均ボリューム(dB)になるように、WAV のステレオ波形データ(16ビット, L/R)をスケールする。
    /// </summary>
    /// <param name="waveFileInOut">
    ///     ステレオ WAV(44.1 kHz, 16bit, 2ch)の生 PCM データ(ヘッダ抜き)。
    ///     音量調整処理を施した結果はこの配列を直接上書きする。
    /// </param>
    /// <param name="targetVolume">目指す平均ボリューム(dB)。CalcMeanVolume の戻り値と同じ定義。</param>
    /// <param name="cancel">中断用トークン</param>
    public static void AdjustVolume(Memory<byte> waveFileInOut, double targetVolume, CancellationToken cancel)
    {
        // まず現在の平均ボリューム(dB)を測る
        double currentVolumeDb = CalcMeanVolume(waveFileInOut, cancel);

        // dB 差分
        double deltaDb = targetVolume - currentVolumeDb;
        // 線形スケール倍率 = 10^(deltaDb/20)
        double scale = Math.Pow(10.0, deltaDb / 20.0);

        // スケールが 1.0 とほぼ同じなら何もしない
        if (Math.Abs(scale - 1.0) < 1.0e-8)
        {
            return;
        }

        // 全サンプルにスケールをかけて書き戻す
        int frameCount = waveFileInOut.Length / 4;
        var span = waveFileInOut.Span;

        for (int i = 0; i < frameCount; i++)
        {
            cancel.ThrowIfCancellationRequested();

            short left = (short)((span[4 * i + 1] << 8) | span[4 * i + 0]);
            short right = (short)((span[4 * i + 3] << 8) | span[4 * i + 2]);

            // スケーリング (float化して乗算し、clamp)
            double newLeft = left * scale;
            double newRight = right * scale;

            short leftScaled = (short)Math.Clamp((int)Math.Round(newLeft), short.MinValue, short.MaxValue);
            short rightScaled = (short)Math.Clamp((int)Math.Round(newRight), short.MinValue, short.MaxValue);

            // 書き戻し (リトルエンディアン)
            span[4 * i + 0] = (byte)(leftScaled & 0xFF);
            span[4 * i + 1] = (byte)((leftScaled >> 8) & 0xFF);
            span[4 * i + 2] = (byte)(rightScaled & 0xFF);
            span[4 * i + 3] = (byte)((rightScaled >> 8) & 0xFF);
        }
    }
}

[Flags]
public enum AiAudioEffectSpeedType
{
    Light = 0,
    Normal = 1,
    Heavy = 2,
}

public static class AiAudioEffectCollection
{
    public static IReadOnlyList<AiAudioEffectBase> AllCollectionList { get; }
    public static ShuffledEndlessQueue<AiAudioEffectBase> AllCollectionRandomQueue { get; }

    static AiAudioEffectCollection()
    {
        List<AiAudioEffectBase> allc = new();

        allc.Add(new AiAudioEffect_02_NearFar());
        allc.Add(new AiAudioEffect_02_NearFar());
        allc.Add(new AiAudioEffect_04_FreeFall());
        allc.Add(new AiAudioEffect_05_Flap());
        allc.Add(new AiAudioEffect_07_Tremolo());
        allc.Add(new AiAudioEffect_07_Tremolo());

        AllCollectionList = allc;

        AllCollectionRandomQueue = new(allc);
    }
}

public interface IAiAudioEffectSettings
{
}

public abstract class AiAudioEffectBase
{
    protected abstract IAiAudioEffectSettings NewSettingsFactoryImpl();
    protected abstract IAiAudioEffectSettings NewSettingsFactoryWithRandomImpl(AiAudioEffectSpeedType type);
    protected abstract void ProcessFilterImpl(Memory<byte> waveFileInOut, IAiAudioEffectSettings effectSettings, CancellationToken cancel);

    public IAiAudioEffectSettings NewSettingsFactory() => NewSettingsFactoryImpl();
    public IAiAudioEffectSettings NewSettingsFactoryWithRandom(AiAudioEffectSpeedType type = AiAudioEffectSpeedType.Normal)
    {
        if (type == AiAudioEffectSpeedType.Heavy) type = AiAudioEffectSpeedType.Normal; // 当面
        return NewSettingsFactoryWithRandomImpl(type);
    }
    public void ProcessFilter(Memory<byte> waveFileInOut, IAiAudioEffectSettings settings, CancellationToken cancel = default) => ProcessFilterImpl(waveFileInOut, settings, cancel);
    public void ProcessFilterRandom(Memory<byte> waveFileInOut, AiAudioEffectSpeedType type, CancellationToken cancel = default) => ProcessFilter(waveFileInOut, NewSettingsFactoryWithRandom(type), cancel);
}


#region AiAudioEffect_02_NearFar_Settings
// このクラスが、処理の内容 (効果の程度や挙動の変化) を決定する枢要なパラメータ指定部分である。
public class AiAudioEffect_02_NearFar_Settings : IAiAudioEffectSettings
{
    /*
     * 【各パラメータの説明】
     * 
     * ■ MaxSpeed
     *    - 音源がランダムに移動する際の、速度ベクトルの最大値 (単位: 「リスナーを中心とする仮想空間上」での 1秒あたりの移動量)。
     *    - 値を大きくすると、音源の動きが激しくなり、高速で近づいてきたり去っていく挙動になる。
     *    - デフォルト値 3.0 は「そこそこの速度感」を想定した値。
     */
    public double MaxSpeed = 3.0;

    /*
     * ■ MovementChangeInterval
     *    - 音源の移動速度 (方向・大きさ) をランダムに変更する周期 (秒単位)。
     *    - 値を小さくすると、より頻繁に方向転換したり、ランダムさが増して落ち着かない感じになる。
     *    - デフォルト値 0.5 は「1秒に2回ほどの頻度でランダムに動きが変化する」ことを想定。
     */
    public double MovementChangeInterval = 0.5;

    /*
     * ■ MinDistance
     *    - 音源がリスナー (中心) に近づいてくるときの最小距離。これ以上は近づきすぎないようにする。
     *    - 値を小さくすると、音源がリスナーの真横をかすめるような迫力を大きくできるが、0 に近すぎると
     *      計算で無限大に音量が上がるなどの問題が起きやすい。
     *    - デフォルト値 0.5 は「そこそこ近くまで接近して大きな音量変化を感じる」想定。
     */
    public double MinDistance = 0.5;

    /*
     * ■ MaxDistance
     *    - 音源がリスナー (中心) から遠ざかるときの最大距離。これ以上遠くには行かないようにする。
     *    - 値を大きくすると、音源が遠方に離れる時間が増えるため、音量が小さめになる時間が長くなる。
     *    - デフォルト値 5.0 は「ある程度遠くまで離れていく」想定。
     */
    public double MaxDistance = 5.0;

    /*
     * ■ EnablePitchMod
     *    - ドップラー効果のような、微妙なピッチ変化を擬似的に入れるかどうかのフラグ。
     *    - true なら、近づく際にピッチが上がり、遠ざかる際にピッチが下がる簡易的効果を入れる。
     *    - デフォルト値は true。不要な場合は false。
     */
    public bool EnablePitchMod = true;

    /*
     * ■ PitchModIntensity
     *    - EnablePitchMod=true の時に、有効となるピッチ変化の強度 (±何%までピッチを変化させるか) を決める値。
     *    - たとえば 0.03 なら、±3% 程度のピッチ変化をランダムに加える。
     *    - デフォルト値 0.03。
     */
    public double PitchModIntensity = 0.03;
}

// このクラスが、処理の内容を実装するクラスである。
public class AiAudioEffect_02_NearFar : AiAudioEffectBase
{
    // この関数は、そのまま、書き写すこと。
    protected override IAiAudioEffectSettings NewSettingsFactoryImpl()
        => new AiAudioEffect_02_NearFar_Settings();

    // 効果のバリエーションを増すためのパラメータのランダム設定を、おおまかな、「強・注・弱」の効果の強さの程度の指定をある程度もとにして、助ける関数
    protected override IAiAudioEffectSettings NewSettingsFactoryWithRandomImpl(AiAudioEffectSpeedType type)
    {
        var ret = new AiAudioEffect_02_NearFar_Settings();

        /*
         * ここでは、type の値に従って、Random.Shared を使いながらランダムパラメータを設定している。
         * 「強」「中」「弱」に応じておおまかに以下のように変化させている：
         *   - Heavy: 激しく音源が動き回り、近接や遠方の変化も大きめ
         *   - Normal: 中程度
         *   - Light: ゆったりした移動と近接・遠方変化は控えめ
         */
        switch (type)
        {
            case AiAudioEffectSpeedType.Heavy:
                ret.MaxSpeed = Random.Shared.NextDouble() * (10.0 - 5.0) + 5.0;         // 5.0 ～ 10.0
                ret.MovementChangeInterval = Random.Shared.NextDouble() * (0.7 - 0.2) + 0.2; // 0.2 ～ 0.7
                ret.MinDistance = Random.Shared.NextDouble() * (0.7 - 0.2) + 0.2;    // 0.2 ～ 0.7
                ret.MaxDistance = Random.Shared.NextDouble() * (7.0 - 4.0) + 4.0;    // 4.0 ～ 7.0
                ret.EnablePitchMod = (Random.Shared.NextDouble() < 0.9); // 90% の確率で有効に
                ret.PitchModIntensity = Random.Shared.NextDouble() * (0.07 - 0.03) + 0.03; // 0.03 ～ 0.07
                break;

            case AiAudioEffectSpeedType.Normal:
                ret.MaxSpeed = Random.Shared.NextDouble() * (5.0 - 2.0) + 2.0;        // 2.0 ～ 5.0
                ret.MovementChangeInterval = Random.Shared.NextDouble() * (1.0 - 0.5) + 0.5; // 0.5 ～ 1.0
                ret.MinDistance = Random.Shared.NextDouble() * (1.0 - 0.3) + 0.3;     // 0.3 ～ 1.0
                ret.MaxDistance = Random.Shared.NextDouble() * (6.0 - 3.0) + 3.0;     // 3.0 ～ 6.0
                ret.EnablePitchMod = (Random.Shared.NextDouble() < 0.6); // 60% の確率で有効に
                ret.PitchModIntensity = Random.Shared.NextDouble() * (0.05 - 0.01) + 0.01; // 0.01 ～ 0.05
                break;

            case AiAudioEffectSpeedType.Light:
                ret.MaxSpeed = Random.Shared.NextDouble() * (2.0 - 0.3) + 0.3;        // 0.3 ～ 2.0
                ret.MovementChangeInterval = Random.Shared.NextDouble() * (1.5 - 0.8) + 0.8; // 0.8 ～ 1.5
                ret.MinDistance = Random.Shared.NextDouble() * (1.2 - 0.5) + 0.5;     // 0.5 ～ 1.2
                ret.MaxDistance = Random.Shared.NextDouble() * (3.0 - 1.5) + 1.5;     // 1.5 ～ 3.0
                ret.EnablePitchMod = (Random.Shared.NextDouble() < 0.3); // 30% の確率で有効に
                ret.PitchModIntensity = Random.Shared.NextDouble() * (0.03 - 0.005) + 0.005; // 0.005 ～ 0.03
                break;
        }

        return ret;
    }

    // この関数が、「効果」を生み出す処理を実際に行なう枢要部分である。
    protected override void ProcessFilterImpl(Memory<byte> waveFileInOut, IAiAudioEffectSettings effectSettings, CancellationToken cancel)
    {
        // まず、effectSettings の内容を、扱いやすいようにキャストする。
        AiAudioEffect_02_NearFar_Settings settings = (AiAudioEffect_02_NearFar_Settings)effectSettings;

        /* 
         * 実装部分ここから 
         *
         * WAV データ (44.1kHz, 16bit, ステレオ, ヘッダなし) に対して、
         * 「高速で音源が近付いてきて、左右にかすめたり、ランダムな動きでびっくりさせるようなエフェクト」を付与する。
         * 具体的には：
         *   1) リスナー (中心) を原点 (0,0) とする仮想空間で、音源がランダムな位置に存在し、
         *      ランダムなベクトルで動き回るのをシミュレーションし、サンプルごとに位置を更新。
         *   2) 位置 (x,y) から求めた「距離」に応じて音量を変化 (近づくと大きく、遠ざかると小さく)。
         *   3) 位置 (x) に応じてステレオの左右バランスを変化 (左にあるほど Left チャンネルを強め、Right を弱める)。
         *   4) 必要であれば、ドップラー効果っぽいピッチ変化も導入 (EnablePitchMod が true の場合)。
         *   5) 一定間隔 (MovementChangeInterval 秒) ごとに、速度ベクトルをランダムに変更して予測不能な移動を演出。
         *   6) MinDistance、MaxDistance の範囲を超えないように位置を補正 (または簡易的に反転など)。
         * 
         * 下記では、計算量が多くなるため、巨大なループ処理中に cancel.ThrowIfCancellationRequested() を適宜挿入し、
         * 中断要求に応じられるようにしている。
         */

        // PCM 16bit ステレオの場合、1サンプルあたり4バイト (L, R 各16bit)
        int bytesPerSampleFrame = 4;
        int totalSampleFrames = waveFileInOut.Length / bytesPerSampleFrame;

        // サンプリングレート (44.1kHz)
        double sampleRate = 44100.0;

        // 仮想空間上の音源位置と速度を用意する
        //   とりあえず初期位置は (0, settings.MinDistance) あたりに置いておく(必ずしもここである必要はない)。
        //   速度ベクトルも、最初はランダムに決める。
        double x = 0.0;
        double y = settings.MinDistance + 0.1; // 少しだけ離れたところから始める
        Random rand = new Random();

        // 速度ベクトルをランダム生成するヘルパー関数
        Func<(double vx, double vy)> getRandomVelocity = () =>
        {
            // -MaxSpeed ～ MaxSpeed の範囲でランダムな速度を生成
            double vx = (rand.NextDouble() * 2.0 - 1.0) * settings.MaxSpeed;
            double vy = (rand.NextDouble() * 2.0 - 1.0) * settings.MaxSpeed;
            return (vx, vy);
        };

        (double vx, double vy) = getRandomVelocity();

        // ピッチ変化用のサンプル再生位置管理 (ナイーブなピッチシフタ)
        //   実際のドップラー計算を簡略化し、「接近時は再生速度を少し上げ、離れるときは下げる」といった実装例。
        //   ただし、高品位なピッチシフタではないため、音質の劣化はあり得る。
        double playbackIndex = 0.0; // 現在再生するサンプルフレームの浮動小数位置
        double pitchDelta = 0.0;    // ドップラーによるピッチ変化量

        // 次に速度を変化させる目標時刻 (秒)
        double nextChangeTime = settings.MovementChangeInterval;
        double currentTime = 0.0; // 処理経過時間(秒)
        double dt = 1.0 / sampleRate; // サンプルフレーム間の経過秒数

        // 波形アクセス用の Span
        var span = waveFileInOut.Span;

        // 入力波形を 16bit ステレオとして読み取りながら、書き込みも行うため、元のサンプルを
        // 一時バッファに退避しておき、それを都度読み出す手法を取る。
        // → 大きなメモリ使用になる可能性があるので注意。(ファイルが大きい場合は別手段も検討)
        //   ここではサンプルとして、単純実装のためにすべてロードする。
        short[] originalLeft = new short[totalSampleFrames];
        short[] originalRight = new short[totalSampleFrames];

        // まず元データをコピー
        for (int i = 0; i < totalSampleFrames; i++)
        {
            short l = BitConverter.ToInt16(span.Slice(i * bytesPerSampleFrame, 2));
            short r = BitConverter.ToInt16(span.Slice(i * bytesPerSampleFrame + 2, 2));
            originalLeft[i] = l;
            originalRight[i] = r;
        }

        // 出力先のバッファを 0 クリアしておく (あとで上書き)
        for (int i = 0; i < totalSampleFrames; i++)
        {
            span[i * bytesPerSampleFrame + 0] = 0;
            span[i * bytesPerSampleFrame + 1] = 0;
            span[i * bytesPerSampleFrame + 2] = 0;
            span[i * bytesPerSampleFrame + 3] = 0;
        }

        // メインループ: 出力フレームを 0 から totalSampleFrames-1 まで書き込む
        //   ただし、ピッチ変更 (再生速度変更) を伴うため、
        //   再生する「元データの位置」(playbackIndex) は必ずしも i と等しくない。
        for (int i = 0; i < totalSampleFrames; i++)
        {
            // 中断要求があればここで例外をスロー
            cancel.ThrowIfCancellationRequested();

            // 現在時刻が次の変更時刻を超えたら、速度を再度ランダムに変化させる
            if (currentTime >= nextChangeTime)
            {
                (vx, vy) = getRandomVelocity();
                nextChangeTime += settings.MovementChangeInterval;
            }

            // (1) 位置 x,y を更新
            x += vx * dt;
            y += vy * dt;

            // (2) MinDistance ～ MaxDistance の範囲を超えないように補正 (単純に境界で反射するなど)
            double dist = Math.Sqrt(x * x + y * y);
            if (dist < settings.MinDistance)
            {
                // 小さすぎるので、境界へ押し戻す + 速度ベクトル反転
                double ratio = settings.MinDistance / dist;
                x *= ratio;
                y *= ratio;
                vx = -vx;
                vy = -vy;
            }
            else if (dist > settings.MaxDistance)
            {
                // 大きすぎるので、境界へ押し戻す + 速度ベクトル反転
                double ratio = settings.MaxDistance / dist;
                x *= ratio;
                y *= ratio;
                vx = -vx;
                vy = -vy;
            }
            dist = Math.Sqrt(x * x + y * y); // 再計算

            // (3) ボリュームを計算 (1 / 距離) 程度 (極端になり過ぎないよう、適宜スケール調整もあり)
            double volume = 1.0 / (dist + 0.01); // dist が 0 に近づいた時に無限大にならぬよう微小値を足す

            // (4) ステレオパンを計算: x が負→左側、x が正→右側。
            //     -1.0 ～ +1.0 の範囲のパン値 (pan) としてみる
            //     左ボリューム = volume * (1 - pan)/2
            //     右ボリューム = volume * (1 + pan)/2
            //   pan を x / dist とし、範囲 -1～1 を確保
            double pan = 0.0;
            if (dist > 0.00001)
            {
                pan = Math.Max(-1.0, Math.Min(1.0, x / dist));
            }
            double leftGain = volume * (1.0 - pan) * 0.5;
            double rightGain = volume * (1.0 + pan) * 0.5;

            // (5) ピッチ変化
            //     接近中 (vx*x + vy*y < 0) ならピッチを高く、遠ざかり中なら低くする簡易演算。
            //     vx*x + vy*y は速度ベクトルと位置ベクトルの内積 (正なら遠ざかり, 負なら接近)。
            if (settings.EnablePitchMod)
            {
                double dot = vx * x + vy * y; // 内積
                                              // dot < 0 → 接近, dot > 0 → 遠ざかり
                                              // 接近ならピッチを上げる (playbackIndex を進めるスピードを少し速める)
                                              // 遠ざかりならピッチを下げる
                double sign = (dot < 0.0) ? -1.0 : 1.0;
                // ± settings.PitchModIntensity 程度のピッチ変化を与える
                pitchDelta = 1.0 + sign * settings.PitchModIntensity;
            }
            else
            {
                pitchDelta = 1.0; // ピッチ変化なし
            }

            // (6) 元データのサンプルを再生 (pitchDelta の分だけ読み取り速度を変える)
            //     playbackIndex は浮動小数なので補間が必要だが、ここでは最も単純な nearest neighbor
            //     (または floor) で実装する例を示す。
            int srcIndex = (int)Math.Floor(playbackIndex);
            if (srcIndex < 0) srcIndex = 0;
            if (srcIndex >= totalSampleFrames)
            {
                // もし再生位置が元データを超えてしまったら、ここではループ終了とする。
                // (あるいはループ再生したければ modulo 演算などを使う)
                break;
            }

            short srcL = originalLeft[srcIndex];
            short srcR = originalRight[srcIndex];

            // (7) ボリューム・パン適用
            double outL = srcL * leftGain;
            double outR = srcR * rightGain;

            // 16bitにクリップ
            short finalL = (short)Math.Max(short.MinValue, Math.Min(short.MaxValue, (int)(outL)));
            short finalR = (short)Math.Max(short.MinValue, Math.Min(short.MaxValue, (int)(outR)));

            // (8) waveFileInOut (span) に書き込む (リトルエンディアン)
            BitConverter.TryWriteBytes(span.Slice(i * bytesPerSampleFrame, 2), finalL);
            BitConverter.TryWriteBytes(span.Slice(i * bytesPerSampleFrame + 2, 2), finalR);

            // (9) 次のサンプルへ向けて時刻とピッチ付き再生位置を更新
            currentTime += dt;
            playbackIndex += pitchDelta; // シンプルにピッチ倍率を足していく (これだと加速度的にずれていくので注意)
        }

        /* 実装部分ここまで */
    }
}

#endregion



#region AiAudioEffect_03_JetCoaster
// このクラスが、処理の内容 (効果の程度や挙動の変化) を決定する枢要なパラメータ指定部分である。
public class AiAudioEffect_03_JetCoaster_Settings : IAiAudioEffectSettings
{
    /* 実装部分ここから */
    /// <summary>
    /// 音像が左右に行き来する際の基本的な周波数(Hz)。
    /// 例: 1.5 は、1.5Hz(約0.66秒周期)で左右を往復するイメージになる。
    /// デフォルト: 1.5
    /// </summary>
    public double PanFrequency = 1.5;

    /// <summary>
    /// 左右振り幅の強度(0.0～1.0程度推奨)。
    /// 0.0 は左右差なし、1.0 は最大振幅(片側が完全に小さくなりもう片側が最大になるイメージ)。
    /// デフォルト: 0.7
    /// </summary>
    public double PanIntensity = 0.7;

    /// <summary>
    /// 「上下に急加速・急落下する」ような振動感(ジェットコースター感)を与える周波数(Hz)。
    /// 例: 2.0 にすると、2Hz(0.5秒周期)で振動が上下に繰り返される。
    /// デフォルト: 2.0
    /// </summary>
    public double RollerCoasterFrequency = 2.0;

    /// <summary>
    /// ジェットコースター感の振動の強度(0.0～1.0程度推奨)。
    /// 振動が音量や左右差にどの程度影響を与えるかを制御する。
    /// デフォルト: 0.5
    /// </summary>
    public double RollerCoasterIntensity = 0.5;

    /// <summary>
    /// ある程度の「予測不能性」を与えるために導入する乱数変動の大きさ(0.0以上)。
    /// 値が大きいほどランダム変動が激しくなる。デフォルト: 0.3
    /// </summary>
    public double RandomJerkiness = 0.3;

    /// <summary>
    /// ステレオの左右チャンネルをあえて逆相(フェーズ反転)にするかどうかのフラグ。
    /// 真の場合は、左右の音を逆位相にして不思議な広がり感を出す。
    /// デフォルト: false
    /// </summary>
    public bool ReversePhase = false;
    /* 実装部分ここまで */
}

// このクラスが、処理の内容を実装するクラスである。
public class AiAudioEffect_03_JetCoaster : AiAudioEffectBase
{
    // この関数は、そのまま、書き写すこと。
    protected override IAiAudioEffectSettings NewSettingsFactoryImpl() => new AiAudioEffect_03_JetCoaster_Settings();

    // 効果のバリエーションを増すためのパラメータのランダム設定を、おおまかな、「強・注・弱」の効果の強さの程度の指定をある程度もとにして、助ける関数
    protected override IAiAudioEffectSettings NewSettingsFactoryWithRandomImpl(AiAudioEffectSpeedType type)
    {
        var ret = new AiAudioEffect_03_JetCoaster_Settings();

        /* 実装部分ここから */
        // type の値に応じて、パラメータをランダムに設定する。
        // Heavy: 強い効果・振り幅が大きくランダムも派手
        // Normal: 中程度
        // Light: 穏やか
        // サンプルとして PanFrequency, PanIntensity, RollerCoasterFrequency, RollerCoasterIntensity, RandomJerkiness の範囲を調整
        switch (type)
        {
            case AiAudioEffectSpeedType.Heavy:
                // 左右パンの揺れ(周波数と強度)を広めに乱数設定
                ret.PanFrequency = Random.Shared.NextDouble() * (2.0 - 0.5) + 0.5;  // 0.5 ～ 2.0
                ret.PanIntensity = Random.Shared.NextDouble() * (0.8 - 0.4) + 0.4; // 0.4 ～ 0.8
                                                                                   // ジェットコースター振動
                ret.RollerCoasterFrequency = Random.Shared.NextDouble() * (2.5 - 1.0) + 1.0; // 1.0 ～ 2.5
                ret.RollerCoasterIntensity = Random.Shared.NextDouble() * (0.7 - 0.3) + 0.3; // 0.3 ～ 0.7
                                                                                             // 乱数変動
                ret.RandomJerkiness = Random.Shared.NextDouble() * (0.5 - 0.1) + 0.1; // 0.1 ～ 0.5
                                                                                      // ReversePhase は 30% くらい
                ret.ReversePhase = (Random.Shared.NextDouble() < 0.3);
                break;

            case AiAudioEffectSpeedType.Normal:
                // 左右パン
                ret.PanFrequency = Random.Shared.NextDouble() * (1.5 - 0.4) + 0.4;  // 0.5 ～ 2.0
                ret.PanIntensity = Random.Shared.NextDouble() * (0.6 - 0.3) + 0.3; // 0.4 ～ 0.8
                                                                                   // ジェットコースター振動
                ret.RollerCoasterFrequency = Random.Shared.NextDouble() * (2.0 - 0.7) + 0.7; // 1.0 ～ 2.5
                ret.RollerCoasterIntensity = Random.Shared.NextDouble() * (0.6 - 0.2) + 0.2; // 0.3 ～ 0.7
                                                                                             // 乱数変動
                ret.RandomJerkiness = Random.Shared.NextDouble() * (0.4 - 0.05) + 0.05; // 0.1 ～ 0.5
                                                                                        // ReversePhase は 30% くらい
                ret.ReversePhase = (Random.Shared.NextDouble() < 0.2);
                break;

            case AiAudioEffectSpeedType.Light:
                // 左右パン
                ret.PanFrequency = Random.Shared.NextDouble() * (1.2 - 0.2) + 0.2; // 0.2 ～ 1.2
                ret.PanIntensity = Random.Shared.NextDouble() * (0.5 - 0.2) + 0.2; // 0.2 ～ 0.5
                                                                                   // ジェットコースター振動
                ret.RollerCoasterFrequency = Random.Shared.NextDouble() * (1.5 - 0.5) + 0.5; // 0.5 ～ 1.5
                ret.RollerCoasterIntensity = Random.Shared.NextDouble() * (0.5 - 0.1) + 0.1; // 0.1 ～ 0.5
                                                                                             // 乱数変動
                ret.RandomJerkiness = Random.Shared.NextDouble() * (0.3 - 0.0) + 0.0; // 0.0 ～ 0.3
                                                                                      // ReversePhase は 10% くらい
                ret.ReversePhase = (Random.Shared.NextDouble() < 0.1);
                break;
        }
        /* 実装部分ここまで */

        return ret;
    }

    // この関数が、「効果」を生み出す処理を実際に行なう枢要部分である。
    protected override void ProcessFilterImpl(Memory<byte> waveFileInOut, IAiAudioEffectSettings effectSettings, CancellationToken cancel)
    {
        // まず、effectSettings の内容を、扱いやすいように、AiAudioEffect_03_JetCoaster_Settings にキャストする。
        AiAudioEffect_03_JetCoaster_Settings settings = (AiAudioEffect_03_JetCoaster_Settings)effectSettings;

        /* 実装部分ここから */
        // 44.1kHz, 16bit, ステレオPCMデータと仮定
        // 1サンプル(フレーム)あたり4バイト(Left 2byte + Right 2byte)
        // NAudio.Wave を使ってもよいし、BitConverter でもよい。

        // メモ:
        //   short 左チャンネル = BitConverter.ToInt16(data, index);
        //   short 右チャンネル = BitConverter.ToInt16(data, index + 2);
        //
        // waveFileInOut.Length は波形PCM部分(ヘッダを除いた実データ)のバイト数
        // サンプル数は waveFileInOut.Length / 4 になる(ステレオなので左+右で4バイト)。

        var data = waveFileInOut.Span;
        int totalBytes = data.Length;
        int sampleCount = totalBytes / 4; // ステレオ1フレームあたり4バイト

        // サンプリングレート(44.1kHz)
        const double sampleRate = 44100.0;

        // ここで時間を進めながら、左右の音をジェットコースター風に動かす。
        // 下記では簡単に:
        //   t: 現在の時間(秒) = i / 44100.0
        //   左右パン(左右の振り幅)と、ジェットコースター(上下の揺れ)を混合。
        //   さらに乱数的な揺らぎを加えて、予測不能性を演出する。
        //
        //   - PanLFO = sin(2π * PanFrequency * t)
        //   - CoasterLFO = sin(2π * RollerCoasterFrequency * t)
        //   - それぞれに Intensity (強度) をかける。
        //   - RandomJerkiness 分だけ、一定サンプルごとにランダム値を加算して揺らぎを出す。
        //
        //   出音レベル(振幅) = 元のサンプル値 * [1 + (PanIntensity * PanLFO * panning係数)]
        //   ただし、Left と Right で panning係数 を異なる符号にして左右差を出す。
        //   さらに全体の音量に (1 + RollerCoasterIntensity * CoasterLFO + ランダム揺らぎ) を乗算。
        //   ReversePhase が true の場合、左右の位相を反転(Leftに+をかけるならRightに-をかける 等)して不思議な効果を追加。
        //
        //   具体的には:
        //     leftFactor  = volumeBase * (1 - panFactor)
        //     rightFactor = volumeBase * (1 + panFactor)
        //   などとしてもよい。(あるいは PanIntensity 次第でスケールする)
        //
        //   ループが巨大なので、適宜 cancel.ThrowIfCancellationRequested() を挟む。

        // ランダム揺らぎを加えるための変数
        // 「ある程度のサンプル間隔ごと」に突然変動させる実装例
        double randomOffsetPan = 0.0;
        double randomOffsetCoaster = 0.0;
        int nextRandomizeCount = 0; // 0になると新しい乱数を発生させる

        // データ処理ループ
        for (int i = 0; i < sampleCount; i++)
        {
            // 中断要求が来ていないかチェック
            if (i % 1024 == 0) // 処理負荷を考慮して適度な頻度でチェック
            {
                cancel.ThrowIfCancellationRequested();
            }

            // 現在時間(秒)
            double t = i / sampleRate;

            // もし「次に乱数を振るタイミング」に達したら、ランダムな揺らぎ値を更新
            if (nextRandomizeCount <= 0)
            {
                // 左右パン揺らぎ
                randomOffsetPan = (Random.Shared.NextDouble() - 0.5) * settings.RandomJerkiness * 2.0;
                // 上下Coaster揺らぎ
                randomOffsetCoaster = (Random.Shared.NextDouble() - 0.5) * settings.RandomJerkiness * 2.0;
                // 次の更新タイミング(乱数で 100～1000サンプル後)
                nextRandomizeCount = Random.Shared.Next(100, 1000);
            }
            nextRandomizeCount--;

            // LFO計算
            double panLFO = Math.Sin(2.0 * Math.PI * settings.PanFrequency * t) + randomOffsetPan;
            double coasterLFO = Math.Sin(2.0 * Math.PI * settings.RollerCoasterFrequency * t) + randomOffsetCoaster;

            // PanIntensity分だけパンを乗算
            panLFO *= settings.PanIntensity;
            // RollerCoasterIntensity分だけ上下(音量)を乗算
            coasterLFO *= settings.RollerCoasterIntensity;

            // 全体音量変動(1 + coasterLFO) で上下振動
            // ただし、coasterLFO が負の場合は若干音量が下がる。
            double volumeBase = 1.0 + coasterLFO;
            if (volumeBase < 0.0) volumeBase = 0.0; // 音量がマイナスにならないようにクリップ

            // 左右パン: -panLFO ～ +panLFO の範囲
            // 例: panLFO = +0.8 => 左チャンネルは小さく(1 - 0.8=0.2倍)、右チャンネルは大きく(1 + 0.8=1.8倍)
            //     panLFO = -0.8 => 逆
            double leftFactor = volumeBase * (1.0 - panLFO);
            double rightFactor = volumeBase * (1.0 + panLFO);

            // フェーズ反転を考慮
            // ReversePhase が true の場合、左右で位相を逆にしてみる
            // (ここでは簡単に、右チャンネルの符号を反転させる例を示す)
            if (settings.ReversePhase)
            {
                rightFactor = -rightFactor;
            }

            // 元のサンプルを取得 (16bit, ステレオ)
            int byteIndex = i * 4;
            short leftSample = BitConverter.ToInt16(data.Slice(byteIndex, 2));
            short rightSample = BitConverter.ToInt16(data.Slice(byteIndex + 2, 2));

            // 計算結果を反映
            double newLeft = leftSample * leftFactor;
            double newRight = rightSample * rightFactor;

            // 16bit の範囲にクリップ
            short outLeft = (short)Math.Max(short.MinValue, Math.Min(short.MaxValue, Math.Round(newLeft)));
            short outRight = (short)Math.Max(short.MinValue, Math.Min(short.MaxValue, Math.Round(newRight)));

            // 書き戻し
            BitConverter.TryWriteBytes(data.Slice(byteIndex, 2), outLeft);
            BitConverter.TryWriteBytes(data.Slice(byteIndex + 2, 2), outRight);
        }
        /* 実装部分ここまで */
    }
}

#endregion



#region AiAudioEffect_04_FreeFall_Settings
// このクラスが、処理の内容 (効果の程度や挙動の変化) を決定する枢要なパラメータ指定部分である。
public class AiAudioEffect_04_FreeFall_Settings : IAiAudioEffectSettings
{
    /// <summary>
    /// 自由落下のメイン時間(秒)。
    /// ここで設定した時間で「落下エンベロープ」を形成する。大きいほど長い時間かけて落下するイメージになる。
    /// </summary>
    public double FallDurationSec = 2.0;

    /// <summary>
    /// LFO(低周波数振動)の周波数(Hz)。音量やパンに揺らぎを与える。
    /// 値を大きくすると短い周期で揺れるため、より細かくビブラート的な揺れを感じる。
    /// </summary>
    public double LfoFrequency = 1.0;

    /// <summary>
    /// LFOの深さ(0～1程度が目安)。
    /// 音量/パン変化の振幅をどの程度にするかを決める。大きいほど、揺れ幅が大きい。
    /// </summary>
    public double LfoDepth = 0.3;

    /// <summary>
    /// タービュランス(乱流)を有効にするかどうか。true であれば、ランダムな急変動が時々発生する。
    /// </summary>
    public bool EnableTurbulence = true;

    /// <summary>
    /// タービュランスの強度(0～1程度が目安)。
    /// 乱流発生時に、どの程度の変化幅が加わるかを決める。
    /// </summary>
    public double TurbulenceIntensity = 0.4;
}
#endregion

#region AiAudioEffect_04_FreeFall
public class AiAudioEffect_04_FreeFall : AiAudioEffectBase
{
    // そのまま書き写す
    protected override IAiAudioEffectSettings NewSettingsFactoryImpl()
        => new AiAudioEffect_04_FreeFall_Settings();

    // 効果のバリエーションを増すためのパラメータのランダム設定
    protected override IAiAudioEffectSettings NewSettingsFactoryWithRandomImpl(AiAudioEffectSpeedType type)
    {
        var ret = new AiAudioEffect_04_FreeFall_Settings();

        /* 実装部分ここから */
        switch (type)
        {
            case AiAudioEffectSpeedType.Heavy:
                // Heavy: より長く激しい落下、深いLFO、強い乱流
                ret.FallDurationSec = Random.Shared.NextDouble() * (5.0 - 2.0) + 2.0; // 2.0 ～ 5.0
                ret.LfoFrequency = Random.Shared.NextDouble() * (2.0 - 0.5) + 0.5;   // 0.5 ～ 2.0
                ret.LfoDepth = Random.Shared.NextDouble() * (1.0 - 0.5) + 0.5;       // 0.5 ～ 1.0
                ret.EnableTurbulence = Random.Shared.NextDouble() < 0.7;            // 約70%で乱流ON
                ret.TurbulenceIntensity = Random.Shared.NextDouble() * (0.8 - 0.3) + 0.3; // 0.3～0.8
                break;

            case AiAudioEffectSpeedType.Normal:
                // Normal: 中程度の落下と揺れ、そこそこの乱流
                ret.FallDurationSec = Random.Shared.NextDouble() * (3.0 - 1.0) + 1.0; // 1.0 ～ 3.0
                ret.LfoFrequency = Random.Shared.NextDouble() * (1.0 - 0.2) + 0.2;    // 0.2 ～ 1.0
                ret.LfoDepth = Random.Shared.NextDouble() * (0.6 - 0.3) + 0.3;        // 0.3 ～ 0.6
                ret.EnableTurbulence = Random.Shared.NextDouble() < 0.4;             // 約40%で乱流ON
                ret.TurbulenceIntensity = Random.Shared.NextDouble() * (0.5 - 0.2) + 0.2; // 0.2～0.5
                break;

            case AiAudioEffectSpeedType.Light:
                // Light: 短い落下と穏やかな揺れ、乱流はあまり期待しない
                ret.FallDurationSec = Random.Shared.NextDouble() * (1.5 - 0.5) + 0.5;  // 0.5 ～ 1.5
                ret.LfoFrequency = Random.Shared.NextDouble() * (0.5 - 0.1) + 0.1;     // 0.1 ～ 0.5
                ret.LfoDepth = Random.Shared.NextDouble() * (0.3 - 0.1) + 0.1;         // 0.1 ～ 0.3
                ret.EnableTurbulence = Random.Shared.NextDouble() < 0.2;              // 約20%で乱流ON
                ret.TurbulenceIntensity = Random.Shared.NextDouble() * (0.3 - 0.1) + 0.1; // 0.1～0.3
                break;
        }
        /* 実装部分ここまで */

        return ret;
    }

    // WAVデータ(44.1kHz, 16bit, ステレオの波形部分)に「自由落下」効果を与える
    protected override void ProcessFilterImpl(Memory<byte> waveFileInOut, IAiAudioEffectSettings effectSettings, CancellationToken cancel)
    {
        // まず、effectSettings の内容をキャスト
        AiAudioEffect_04_FreeFall_Settings settings = (AiAudioEffect_04_FreeFall_Settings)effectSettings;

        /* 実装部分ここから */

        // パラメータ取得
        double fallDuration = settings.FallDurationSec;   // 自由落下の周期(片道)
        double lfoFreq = settings.LfoFrequency;           // LFO周波数
        double lfoDepth = settings.LfoDepth;              // LFO深さ
        bool enableTurb = settings.EnableTurbulence;      // 乱流フラグ
        double turbIntensity = settings.TurbulenceIntensity; // 乱流強度

        // WAVのサンプリングレート(本タスクでは44.1kHzと仮定)
        const int sampleRate = 44100;
        // ステレオ16bitPCM(Left, Right) = 4バイト/フレーム
        int bytesPerFrame = 4;
        int totalBytes = waveFileInOut.Length;
        int totalFrames = totalBytes / bytesPerFrame; // サンプルフレーム数

        // バイト列を Span で扱う
        var span = waveFileInOut.Span;

        // 乱流イベント関連の制御変数
        //  - 一定時間ごとにランダムで「衝撃」(Amplitude/Pan変化)を起こす例
        //  - nextTurbEventTime: 次の乱流発生予定時刻(秒)
        //  - currentTurbEffect: 乱流が発生している間の係数(瞬間的に加算してだんだん消えるなど)
        double nextTurbEventTime = 0.5;  // 0.5秒後に初回発生を狙う
        double lastTurbTimeSec = -1.0;   // 乱流が起きた時の時刻。-1ならまだ起きていない
        double turbDecayDuration = 0.2;  // 乱流が0.2秒ぐらいかけて減衰するイメージ
        double turbRandomAmp = 0.0;      // 乱流による振幅変化量(ランダム生成)
        double turbRandomPan = 0.0;      // 乱流によるパン変化量(ランダム生成)

        for (int i = 0; i < totalFrames; i++)
        {
            // キャンセル指示が来ていないか、時々チェック
            if (i % 10000 == 0)
            {
                cancel.ThrowIfCancellationRequested();
            }

            // 現在の時刻(秒)
            double timeSec = (double)i / sampleRate;

            // ----------------
            // 1) 自由落下エンベロープを求める
            //    - 2*fallDuration(往復)でひとつのサイクルとし、繰り返す。
            double cyclePeriod = fallDuration * 2.0;
            double cycleTime = timeSec % cyclePeriod; // 現在のサイクル内での経過時間(0～2*fallDuration)
            // 片道: 0～fallDuration の間は落下(1.0→0.3へ線形に減衰するイメージ)、
            // 戻り: fallDuration～2*fallDuration で 0.3→1.0 に戻るイメージ。
            double minAmp = 0.3; // 落下時の最低振幅(ここの値は好みに合わせて調整)
            double baseEnv;
            if (cycleTime <= fallDuration)
            {
                // 落下フェーズ
                double ratio = cycleTime / fallDuration; // 0.0→1.0
                baseEnv = 1.0 - ratio * (1.0 - minAmp);
            }
            else
            {
                // 戻りフェーズ
                double ratio = (cycleTime - fallDuration) / fallDuration; // 0.0→1.0
                baseEnv = minAmp + ratio * (1.0 - minAmp);
            }

            // ----------------
            // 2) LFO による振幅変調 & パン変調
            //    - LFOは sin() を用いて揺らす。
            double lfoPhase = 2.0 * Math.PI * lfoFreq * timeSec;  // LFOの位相
            double lfoVal = Math.Sin(lfoPhase);                   // -1.0～+1.0

            // amplitudeFactor: baseEnv をさらに揺らす。 (1.0±lfoDepth/2 ではなく、ここでは直接掛け合わせる例)
            // ただし揺れすぎを防ぐため、(1 + lfoDepth*lfoVal) のようにする
            double amplitudeFactor = baseEnv * (1.0 + lfoDepth * lfoVal);

            // panningFactor: 左右で -1.0～+1.0 の揺れを作る
            // たとえば lfoVal=-1～+1 をそのまま利用し、深さを掛け算
            double panVal = lfoDepth * lfoVal;  // -lfoDepth ～ +lfoDepth
            // leftGain = 1 - panVal, rightGain = 1 + panVal をベースに正規化する例
            double leftGain = 1.0 - panVal;
            double rightGain = 1.0 + panVal;
            // このままだと左右とも最大が 1+(lfoDepth) になり得るので、ここでは
            // ざっくり 1 / (1 + lfoDepth) で均等化しておく。
            double norm = 1.0 / (1.0 + lfoDepth);
            leftGain *= norm;
            rightGain *= norm;

            // ----------------
            // 3) 乱流(タービュランス)の発生
            //    - 一定時間(乱数)ごとに、急激な衝撃(ampとpanが変化)が短時間起こる
            if (enableTurb)
            {
                if (timeSec >= nextTurbEventTime)
                {
                    // ここで乱流開始
                    lastTurbTimeSec = timeSec;
                    // 乱流の幅をランダム生成
                    turbRandomAmp = (Random.Shared.NextDouble() * 2.0 - 1.0) * turbIntensity; // -turbIntensity ～ +turbIntensity
                    turbRandomPan = (Random.Shared.NextDouble() * 2.0 - 1.0) * turbIntensity; // 同上
                    // 次の乱流イベントを適当な時間後に設定
                    // (例: 0.3秒後～1.5秒後のどこか)
                    double interval = Random.Shared.NextDouble() * (1.5 - 0.3) + 0.3;
                    nextTurbEventTime = timeSec + interval;
                }

                // 乱流が発動してから turbDecayDuration 秒以内なら効果を適用
                if (lastTurbTimeSec >= 0)
                {
                    double dt = timeSec - lastTurbTimeSec;
                    if (dt <= turbDecayDuration)
                    {
                        double decayRatio = 1.0 - (dt / turbDecayDuration); // 1.0→0.0
                        // 振幅に加算
                        amplitudeFactor += turbRandomAmp * decayRatio;
                        // パンにも加算(左右ゲインに影響させる例)
                        double turbPanOffset = turbRandomPan * decayRatio;
                        leftGain += -turbPanOffset;
                        rightGain += turbPanOffset;
                    }
                    else
                    {
                        // 乱流時間が過ぎたら終了
                        lastTurbTimeSec = -1;
                    }
                }
            }

            // ----------------
            // 4) 実際のPCMデータに反映 (16bitステレオ)
            //    - Byte配列 -> short(左), short(右) に変換して読み書き
            int baseIndex = i * bytesPerFrame;
            short leftSample = BitConverter.ToInt16(span.Slice(baseIndex, 2));
            short rightSample = BitConverter.ToInt16(span.Slice(baseIndex + 2, 2));

            // floatやdouble計算に変換
            double left = leftSample;
            double right = rightSample;

            // 音量・パンの適用
            left *= amplitudeFactor * leftGain;
            right *= amplitudeFactor * rightGain;

            // 16bit範囲にクリップ
            if (left > short.MaxValue) left = short.MaxValue;
            if (left < short.MinValue) left = short.MinValue;
            if (right > short.MaxValue) right = short.MaxValue;
            if (right < short.MinValue) right = short.MinValue;

            // 書き戻し
            short newLeftSample = (short)left;
            short newRightSample = (short)right;
            BitConverter.TryWriteBytes(span.Slice(baseIndex, 2), newLeftSample);
            BitConverter.TryWriteBytes(span.Slice(baseIndex + 2, 2), newRightSample);
        }

        /* 実装部分ここまで */
    }
}
#endregion




#region AiAudioEffect_05_Flap
public class AiAudioEffect_05_Flap_Settings : IAiAudioEffectSettings
{
    /* 実装部分ここから
     * 
     * 【パラメータ例】
     *   1) LfoRateHz:
     *      1秒間に何回「左右移動の変化（ランダムパン）」を行うかを指定する。大きいほど素早い変化となり、効果が激しくなる。
     *      デフォルト: 6.0 (1秒間に6回変化)
     *
     *   2) Depth:
     *      パンの強度(振れ幅)を示す。0.0～1.0程度で扱う想定。
     *      1.0に近いほど、左右への振り切りが大きくなる。
     *      デフォルト: 0.8
     *
     *   3) ReturnToCenterProbability:
     *      新しいパン値を決定するときに、センター(=0)に戻す確率(0.0～1.0)を指定する。
     *      大きいほどセンターに戻りやすく、左右に振り切る時間が少なくなる。
     *      デフォルト: 0.1
     *
     *   4) TransitionLengthSec:
     *      次のパン値に移行するときの「なめらかに移行する時間(秒)」を指定する。0.0なら瞬時切り替え。
     *      デフォルト: 0.01 (10msかけて徐々に次のパン値へ移行する)
     *
     *   5) SmoothedTransition:
     *      true の場合、パン値の切り替えを「TransitionLengthSec」にしたがって滑らかに補間する。
     *      false の場合、瞬時にパン値が切り替わり、より激しい効果になる。
     *      デフォルト: true
     */

    public double LfoRateHz = 6.0;
    public double Depth = 0.8;
    public double ReturnToCenterProbability = 0.1;
    public double TransitionLengthSec = 0.01;
    public bool SmoothedTransition = true;

    /* 実装部分ここまで */
}

// このクラスが、処理の内容を実装するクラスである。
public class AiAudioEffect_05_Flap : AiAudioEffectBase
{
    // この関数は、そのまま、書き写すこと。
    protected override IAiAudioEffectSettings NewSettingsFactoryImpl() => new AiAudioEffect_05_Flap_Settings();

    // 効果のバリエーションを増すためのパラメータのランダム設定を、おおまかな、「強・注・弱」の効果の強さの程度の指定をある程度もとにして、助ける関数
    protected override IAiAudioEffectSettings NewSettingsFactoryWithRandomImpl(AiAudioEffectSpeedType type)
    {
        var ret = new AiAudioEffect_05_Flap_Settings();

        /* 実装部分ここから
         *   type の値に従って、ランダムパラメータの設定をする。
         *   Heavy の場合: 強く激しい効果が出るよう値を大きめに
         *   Normal の場合: 中間的な値
         *   Light の場合: 弱めの値
         */
        switch (type)
        {
            case AiAudioEffectSpeedType.Heavy:
                ret.LfoRateHz = 2 * Random.Shared.NextDouble() * (12.0 - 5.0) + 5.0; // 5.0 ～ 12.0
                ret.Depth = Random.Shared.NextDouble() * (1.0 - 0.7) + 0.7;       // 0.7 ～ 1.0
                ret.ReturnToCenterProbability = Random.Shared.NextDouble() * (0.4 - 0.2) + 0.2; // 0.2～0.4
                ret.TransitionLengthSec = Random.Shared.NextDouble() * (0.03 - 0.005) + 0.005;   // 0.005～0.03
                ret.SmoothedTransition = (Random.Shared.NextDouble() < 0.8); // 80%の確率でスムーズ切り替え
                break;

            case AiAudioEffectSpeedType.Normal:
                ret.LfoRateHz = 2 * Random.Shared.NextDouble() * (8.0 - 3.0) + 3.0; // 3.0 ～ 8.0
                ret.Depth = Random.Shared.NextDouble() * (0.8 - 0.4) + 0.4;      // 0.4 ～ 0.8
                ret.ReturnToCenterProbability = Random.Shared.NextDouble() * (0.2 - 0.05) + 0.05; // 0.05～0.2
                ret.TransitionLengthSec = Random.Shared.NextDouble() * (0.02 - 0.005) + 0.005;     // 0.005～0.02
                ret.SmoothedTransition = (Random.Shared.NextDouble() < 0.7);
                break;

            case AiAudioEffectSpeedType.Light:
                ret.LfoRateHz = 2 * Random.Shared.NextDouble() * (3.0 - 1.0) + 1.0; // 1.0 ～ 3.0
                ret.Depth = Random.Shared.NextDouble() * (0.4 - 0.1) + 0.1;      // 0.1 ～ 0.4
                ret.ReturnToCenterProbability = Random.Shared.NextDouble() * (0.1 - 0.0) + 0.0; // 0.0～0.1
                ret.TransitionLengthSec = Random.Shared.NextDouble() * (0.02 - 0.0) + 0.0;       // 0.0～0.02
                ret.SmoothedTransition = (Random.Shared.NextDouble() < 0.5);
                break;
        }
        /* 実装部分ここまで */

        return ret;
    }

    // この関数が、「効果」を生み出す処理を実際に行なう枢要部分である。
    protected override void ProcessFilterImpl(Memory<byte> waveFileInOut, IAiAudioEffectSettings effectSettings, CancellationToken cancel)
    {
        // まず、effectSettings の内容を、扱いやすいように、AiAudioEffect_05_Flap_Settings にキャストする。
        AiAudioEffect_05_Flap_Settings settings = (AiAudioEffect_05_Flap_Settings)effectSettings;

        /* 実装部分ここから */

        // ■ 概要: 
        //   - ステレオ16bit(左右1サンプルあたり2byte計4byte)の PCM データに対して、
        //     ランダムにパン値(panning factor)を変化させ、左右の音量比を変化させることで
        //     頭の中を左右に激しく行き来するようなサイケデリック効果を与える。
        //   - LfoRateHz 回/秒の周期でパン値を更新し、Depth の値に従って左右への振れ幅を調整する。
        //   - ReturnToCenterProbability の確率で、次のパン値を 0(センター) にする。
        //   - SmoothedTransition が true の場合は、TransitionLengthSec 秒かけて
        //     前回のパン値から線形補間して移動する。
        //   - 定期的に cancel.ThrowIfCancellationRequested(); を呼び出すことで中断要求を処理する。

        var span = waveFileInOut.Span;

        // WAV は 44100Hz, 16bit, Stereo(2ch) と仮定
        int bytesPerSample = 2 * 2;  // 2ch × 16bit(2byte)
        int totalSamples = waveFileInOut.Length / bytesPerSample;
        int sampleRate = 44100;

        double lfoRateHz = settings.LfoRateHz;       // パンを切り替える頻度(Hz)
        double depth = Math.Clamp(settings.Depth, 0.0, 1.0); // 左右振れ幅(0.0～1.0)
        double returnCenterProb = Math.Clamp(settings.ReturnToCenterProbability, 0.0, 1.0);
        double transitionSec = Math.Max(0.0, settings.TransitionLengthSec);
        bool smooth = settings.SmoothedTransition;

        // パン値 p は -1.0～+1.0 で扱い、p>0 なら右寄り, p<0 なら左寄り, p=0 ならセンター。
        // ただし、実際の音量への反映は Depth を掛ける。

        // 何サンプルごとにパンを切り替えるか
        // ※連続的に変化し続けるのではなく、サンプル＆ホールド的に一定のパン値を保ち、
        //   次の区間で新しいパン値をランダムに設定するイメージ
        int samplesPerLfo = (lfoRateHz <= 0.0001)
            ? int.MaxValue
            : (int)(sampleRate / lfoRateHz);

        // 直前区間のパン値と、現在区間での目標パン値を保持
        double currentPan = 0.0;
        double nextPan = 0.0;
        // 線形補間用に区間開始サンプル位置、区間終了サンプル位置を保持
        int regionStartSample = 0;
        int regionEndSample = samplesPerLfo;

        // ランダムパン値を求めるサブ関数
        double GetRandomPan()
        {
            // ReturnToCenterProbabilityの確率で、pan=0を返す
            if (Random.Shared.NextDouble() < returnCenterProb)
            {
                return 0.0; // センター
            }
            // そうでなければ -1.0～+1.0 をランダムに返す
            double p = (Random.Shared.NextDouble() * 2.0) - 1.0; // -1～+1
            return p;
        }

        // 最初の次Panを決める
        nextPan = GetRandomPan();

        // メインループ
        for (int i = 0; i < totalSamples; i++)
        {
            cancel.ThrowIfCancellationRequested();

            // 現在のパン値を算出(線形補間または瞬時切替)
            double pan;
            if (smooth)
            {
                // regionStartSample ～ regionStartSample+transitionSamples の間は補間
                // それ以降は nextPan で一定
                int transitionSamples = (int)(transitionSec * sampleRate);
                transitionSamples = Math.Min(transitionSamples, (regionEndSample - regionStartSample));

                if (i < regionStartSample + transitionSamples)
                {
                    // まだ補間中
                    double t = (double)(i - regionStartSample) / Math.Max(1, transitionSamples);
                    pan = currentPan + (nextPan - currentPan) * t;
                }
                else
                {
                    // 補間完了後は nextPan を維持
                    pan = nextPan;
                }
            }
            else
            {
                // 瞬時に切り替え(最初のサンプルで nextPan にセット)
                pan = nextPan;
            }

            // 音声データ読み込み
            int offset = i * bytesPerSample;
            short leftSample = BitConverter.ToInt16(span.Slice(offset, 2));
            short rightSample = BitConverter.ToInt16(span.Slice(offset + 2, 2));

            // short → float計算用 ( -32768～32767 の範囲 )
            float leftF = leftSample;
            float rightF = rightSample;

            // panFactor: 実際に左右にかけるゲイン(Depth考慮)
            //   pan>0 => 右が大きく, 左が小さく
            //   pan<0 => 左が大きく, 右が小さく
            //   pan=0 => 左右比は変更なし(Depthが0でなければ、ちょっとでも変わる実装にすることもできるが、ここではセンター維持)
            // ここではシンプルに以下の式を用いる：
            //   gainL = 1 - depth * max(0,  pan)
            //   gainR = 1 - depth * max(0, -pan)
            double p = Math.Clamp(pan, -1.0, 1.0) * depth;
            double gainL = 1.0 - Math.Max(0.0, p);
            double gainR = 1.0 - Math.Max(0.0, -p);

            // 左右へパンを適用
            float newLeftF = (float)(leftF * gainL);
            float newRightF = (float)(rightF * gainR);

            // 16bit範囲にクリップして書き戻し
            short newLeft = (short)Math.Clamp(Math.Round(newLeftF), short.MinValue, short.MaxValue);
            short newRight = (short)Math.Clamp(Math.Round(newRightF), short.MinValue, short.MaxValue);

            BitConverter.TryWriteBytes(span.Slice(offset, 2), newLeft);
            BitConverter.TryWriteBytes(span.Slice(offset + 2, 2), newRight);

            // 次の LFO 区間に到達したらパン値を更新
            if (i >= regionEndSample - 1)
            {
                // 新しい区間の始まりとしてパン値更新
                currentPan = nextPan;
                nextPan = GetRandomPan();

                regionStartSample = regionEndSample;
                regionEndSample = regionStartSample + samplesPerLfo;

                // 万一、regionEndSample が総サンプル数を超えても特に問題はないが、ここでは超えたままでもOK
                // (ループ終了間際に新パン値が更新されるだけ)
            }
        }

        /* 実装部分ここまで */
    }
}
#endregion

#region AiAudioEffect_06_Playing

// このクラスが、処理の内容 (効果の程度や挙動の変化) を決定する枢要なパラメータ指定部分である。
/// <summary>
/// AiAudioEffect_06_Playing 用の設定パラメータクラス。<br/>
/// ・ <see cref="EffectRate"/>: 1 秒あたりに「変化」(左右や両方のチャンネル) が何回発生するかの平均回数を指定。<br/>
/// ・ <see cref="EffectDurationMs"/>: 1 回の「変化」が何ミリ秒続くかを指定。<br/>
/// ・ <see cref="BothChannelsProbability"/>: 変化時に左右同時に発生する確率。残りは左右いずれか片方のみ。<br/>
/// </summary>
public class AiAudioEffect_06_Playing_Settings : IAiAudioEffectSettings
{
    /* 実装部分ここから */

    /// <summary>
    /// 1 秒あたりに「変化」が何回発生するかの平均回数を指定するパラメータ。<br/>
    /// 例: 2.0 にすると、1 秒あたり約 2 回の変化が入るようになる。
    /// </summary>
    public double EffectRate = 2.0;

    /// <summary>
    /// 1 回の「変化」の長さ(ミリ秒)。<br/>
    /// 例: 100.0 にすると、100ms (0.1秒) 間、左右いずれか、または両方を強調して効果を与える。
    /// </summary>
    public double EffectDurationMs = 100.0;

    /// <summary>
    /// 変化時に「左右同時」に発生する確率 (0.0～1.0)。<br/>
    /// 例: 0.3 にすると、約 30% の確率で左右同時、残り 70% は左右いずれか片方のみになる。
    /// </summary>
    public double BothChannelsProbability = 0.3;

    /* 実装部分ここまで */
}

// このクラスが、処理の内容を実装するクラスである。
public class AiAudioEffect_06_Playing : AiAudioEffectBase
{
    // この関数は、そのまま、書き写すこと。
    protected override IAiAudioEffectSettings NewSettingsFactoryImpl() => new AiAudioEffect_06_Playing_Settings();

    // 効果のバリエーションを増すためのパラメータのランダム設定を、おおまかな、「強・注・弱」の効果の強さの程度の指定をある程度もとにして、助ける関数
    protected override IAiAudioEffectSettings NewSettingsFactoryWithRandomImpl(AiAudioEffectSpeedType type)
    {
        var ret = new AiAudioEffect_06_Playing_Settings();

        /* 実装部分ここから */
        // type に応じて、「変化」がより激しいか穏やかか、発生頻度や持続時間、左右同時発生率を変化させるサンプル実装
        switch (type)
        {
            case AiAudioEffectSpeedType.Heavy:
                // Heavy: 1 秒あたり 5～6 回ほど「変化」が起きるように
                ret.EffectRate = Random.Shared.NextDouble() * (6.0 - 5.0) + 5.0; // 5.0～6.0
                                                                                 // 変化時間は短めに (50～80ms) として素早く連続するように
                ret.EffectDurationMs = Random.Shared.NextDouble() * (80.0 - 50.0) + 50.0; // 50.0～80.0
                                                                                          // 両方チャンネル同時が起こりやすい (0.5～1.0)
                ret.BothChannelsProbability = Random.Shared.NextDouble() * (1.0 - 0.5) + 0.5;
                break;
            case AiAudioEffectSpeedType.Normal:
                // Normal: 1 秒あたり 2～3 回くらい
                ret.EffectRate = Random.Shared.NextDouble() * (3.0 - 2.0) + 2.0; // 2.0～3.0
                                                                                 // 変化時間は中程度 (80～150ms)
                ret.EffectDurationMs = Random.Shared.NextDouble() * (150.0 - 80.0) + 80.0; // 80.0～150.0
                                                                                           // 両方チャンネル同時はそこそこ起こる (0.2～0.6)
                ret.BothChannelsProbability = Random.Shared.NextDouble() * (0.6 - 0.2) + 0.2;
                break;
            case AiAudioEffectSpeedType.Light:
                // Light: 1 秒あたり 0.5～1 回くらい
                ret.EffectRate = Random.Shared.NextDouble() * (1.0 - 0.5) + 0.5; // 0.5～1.0
                                                                                 // 変化時間はやや長め (150～250ms)
                ret.EffectDurationMs = Random.Shared.NextDouble() * (250.0 - 150.0) + 150.0; // 150.0～250.0
                                                                                             // 両方チャンネル同時はあまり起こらない (0.0～0.3)
                ret.BothChannelsProbability = Random.Shared.NextDouble() * 0.3;
                break;
        }
        /* 実装部分ここまで */

        return ret;
    }

    // この関数が、「効果」を生み出す処理を実際に行なう枢要部分である。
    protected override void ProcessFilterImpl(Memory<byte> waveFileInOut, IAiAudioEffectSettings effectSettings, CancellationToken cancel)
    {
        // まず、effectSettings の内容を、扱いやすいように、AiAudioEffect_06_Playing_Settings にキャストする。
        AiAudioEffect_06_Playing_Settings settings = (AiAudioEffect_06_Playing_Settings)effectSettings;

        /* 実装部分ここから */

        // waveFileInOut には、44.1 kHz / 16-bit / 2ch (ステレオ) の PCM データ(ヘッダ除く) が含まれている。
        // 1 サンプル (L, R のセット) あたり、4 バイト (16ビット×2ch)。
        // このデータに対して、「変化」効果を与えるイメージ:
        //  - 1 秒あたり settings.Rate 回ほどランダムタイミングで「変化」(左右片方 or 両方) を行う
        //  - 1 回の変化は settings.DurationMs ミリ秒程度
        //  - 変化が発生しているチャンネルは音量をブーストし、他方チャンネルは少しアテニュエート (減衰)
        //  - フェードイン・フェードアウトで自然につままれているように感じさせる
        //  - ランダムな間隔・ランダムなチャンネル (左右 or 両方) で発生
        //
        // 実装方針:
        //  1) サンプル数を計算
        //  2) 変化をどのタイミングでどのチャンネルに発生させるかの「イベントリスト」を構築
        //  3) イベントリストをもとに、サンプルごとの「左チャンネルの増幅率」「右チャンネルの増幅率」を計算
        //  4) waveFileInOut のサンプルを順次読み書きして、増幅率を掛け合わせて書き戻す
        //
        // メモ: 巨大なループでは適宜 cancel.ThrowIfCancellationRequested() を呼んで中断を検知すること。

        // 1) サンプル数を計算
        var waveData = waveFileInOut.Span;
        int totalBytes = waveData.Length;
        // 16bit(2 bytes) x 2ch = 4 bytes per frame
        // フレーム(ステレオ1サンプル) 数
        int totalFrames = totalBytes / 4;
        if (totalFrames <= 0) return;

        int sampleRate = 44100; // 本タスク前提: 44.1 kHz

        // 2) 変化イベントのリストを構築
        //    - 平均的には Rate 回/秒 の頻度
        //    - 1 回の持続は DurationMs ミリ秒
        //    - イベント開始～終了まで音量を変更
        //    - イベントの間隔は (1 / Rate) 秒を中心にランダムとし、実際には ±50% ほどブレさせる例にする
        List<EffectEvent> events = new();

        double averageIntervalSec = (settings.EffectRate <= 0.0)
            ? 999999.0 // 万が一 Rate=0 のときは実質発生しない
            : (1.0 / settings.EffectRate);

        double currentTimeSec = 0.0;
        double totalDurationSec = (double)totalFrames / sampleRate;

        Random rng = new Random();

        while (currentTimeSec < totalDurationSec)
        {
            // 変化イベントの「開始時刻」を決定
            // 平均は averageIntervalSec、±50% 程度に振れ幅をもたせる
            // (例えば Heavy なら 1/5～1/6秒程度がベース)
            double randFactor = rng.NextDouble() * 1.0 + 0.5; // 0.5～1.5
            double intervalSec = averageIntervalSec * randFactor;

            currentTimeSec += intervalSec;
            if (currentTimeSec >= totalDurationSec) break; // 全体長を超えれば終了

            double startSec = currentTimeSec;
            double durSec = settings.EffectDurationMs / 1000.0;
            double endSec = startSec + durSec;
            if (endSec > totalDurationSec) endSec = totalDurationSec;

            // 変化するチャンネルをランダムに決定
            bool both = (rng.NextDouble() < settings.BothChannelsProbability);
            bool left = both ? true : (rng.NextDouble() < 0.5);
            bool right = both ? true : !left; // 両方でない限り、片方は left なら right=false, left=false なら right=true

            // Frames (サンプル) 単位に変換
            int startFrame = (int)(startSec * sampleRate);
            int endFrame = (int)(endSec * sampleRate);

            var eventObj = new EffectEvent
            {
                StartFrame = startFrame,
                EndFrame = endFrame,
                ChannelLeft = left,
                ChannelRight = right
            };
            events.Add(eventObj);
        }

        // 3) イベントをもとに、サンプルごとの「左・右増幅率」を計算する
        //    ここでは 1.0 (何も変化なし) から、変化中はブースト・他チャンネルをアテニュエートするような
        //    変化をフェードイン・フェードアウトしながら付与する。
        //    メモリを大量に消費する可能性があるため、応答速度を優先して double[] で確保しても良い。
        //    ここではサンプル数分の配列を用意する実装例。
        double[] leftGains = new double[totalFrames];
        double[] rightGains = new double[totalFrames];
        // 全サンプル初期値 = 1.0
        for (int i = 0; i < totalFrames; i++)
        {
            leftGains[i] = 1.0;
            rightGains[i] = 1.0;
        }

        // フェードイン・アウトに使う時間(サンプル数) ここでは 10ms 程度 (441 サンプル) にしてみる
        int fadeSamples = (int)(0.01 * sampleRate);

        // イベントごとに、対応する範囲の leftGains, rightGains を上書き
        foreach (var ev in events)
        {
            int evLen = ev.EndFrame - ev.StartFrame;
            if (evLen <= 0) continue;

            // 変化中のピーク増幅率 (たとえば 1.8 倍)
            // 変化中でないチャンネルのアテニュエート率 (たとえば 0.5 倍)
            // 適宜好みで調整可能
            double effectBoost = 1.8;
            double unsetAttenuation = 0.5;

            for (int f = ev.StartFrame; f < ev.EndFrame; f++)
            {
                if (f < 0 || f >= totalFrames) continue;

                // cancel チェック (大きいループなのでたまに確認)
                if ((f % 10000) == 0)
                {
                    cancel.ThrowIfCancellationRequested();
                }

                // フェードイン・アウトによる乗算係数 (0.0～1.0)
                double fadeFactor = 1.0;
                int posInEvent = f - ev.StartFrame;
                int remainInEvent = ev.EndFrame - f - 1;

                // フェードイン: 開始～fadeSamples にかけて線形で 0→1
                if (posInEvent < fadeSamples)
                {
                    fadeFactor *= (double)posInEvent / fadeSamples;
                }
                // フェードアウト: 終了手前 fadeSamples にかけて線形で 1→0
                if (remainInEvent < fadeSamples)
                {
                    fadeFactor *= (double)remainInEvent / fadeSamples;
                }

                // 左チャンネル
                if (ev.ChannelLeft)
                {
                    // 変化対象 => ブースト
                    double current = leftGains[f];
                    current *= (1.0 + (effectBoost - 1.0) * fadeFactor);
                    leftGains[f] = current;
                }
                else
                {
                    // 変化対象外 => アテニュエート
                    double current = leftGains[f];
                    current *= (1.0 - (1.0 - unsetAttenuation) * fadeFactor);
                    leftGains[f] = current;
                }

                // 右チャンネル
                if (ev.ChannelRight)
                {
                    // 変化対象 => ブースト
                    double current = rightGains[f];
                    current *= (1.0 + (effectBoost - 1.0) * fadeFactor);
                    rightGains[f] = current;
                }
                else
                {
                    // 変化対象外 => アテニュエート
                    double current = rightGains[f];
                    current *= (1.0 - (1.0 - unsetAttenuation) * fadeFactor);
                    rightGains[f] = current;
                }
            }
        }

        // 4) 実際に waveFileInOut の各サンプルに対して上記の増幅率を掛け合わせて書き戻す
        //    16bit short の範囲内にクリップする
        const short minVal = short.MinValue; // -32768
        const short maxVal = short.MaxValue; // 32767

        for (int frameIndex = 0; frameIndex < totalFrames; frameIndex++)
        {
            cancel.ThrowIfCancellationRequested();

            // waveFileInOut は (L16bit, R16bit) 順で交互に入っている
            int byteIndex = frameIndex * 4;
            // 左チャンネルの short 値を取得 (リトルエンディアン)
            short sampleLeft = (short)(waveData[byteIndex] | (waveData[byteIndex + 1] << 8));
            // 右チャンネル
            short sampleRight = (short)(waveData[byteIndex + 2] | (waveData[byteIndex + 3] << 8));

            // 増幅率を掛ける
            double outLeft = sampleLeft * leftGains[frameIndex];
            double outRight = sampleRight * rightGains[frameIndex];

            // 16bit にクリップ
            short finalLeft = (short)Math.Clamp((int)Math.Round(outLeft), minVal, maxVal);
            short finalRight = (short)Math.Clamp((int)Math.Round(outRight), minVal, maxVal);

            // 書き戻し
            waveData[byteIndex] = (byte)(finalLeft & 0xFF);
            waveData[byteIndex + 1] = (byte)((finalLeft >> 8) & 0xFF);

            waveData[byteIndex + 2] = (byte)(finalRight & 0xFF);
            waveData[byteIndex + 3] = (byte)((finalRight >> 8) & 0xFF);
        }

        /* 実装部分ここまで */
    }

    /// <summary>
    /// 変化イベント(開始フレーム、終了フレーム、左右チャンネルフラグ) の情報を保持する簡易クラス
    /// </summary>
    private class EffectEvent
    {
        public int StartFrame;
        public int EndFrame;
        public bool ChannelLeft;
        public bool ChannelRight;
    }
}
#endregion



#region AiAudioEffect_07_Tremolo

/// <summary>
/// トレモロおよびオートパン(左右振動)エフェクトを付与するための設定値クラス。
/// </summary>
public class AiAudioEffect_07_Tremolo_Settings : IAiAudioEffectSettings
{
    /// <summary>
    /// LFO (音量変調を行う低周波オシレータ) の周波数 [Hz]。
    /// 例: 5.0 ならば、1秒に5回の振幅変化を行う。
    /// デフォルト: 5.0
    /// </summary>
    public double LfoRateHz = 5.0;

    /// <summary>
    /// トレモロの深さ (変調幅) [0.0～1.0]。1.0 だと音量がゼロまで下がる可能性がある。
    /// 0.0 で変化なし、0.5 で変化量は半分程度。
    /// デフォルト: 0.5
    /// </summary>
    public double TremoloDepth = 0.5;

    /// <summary>
    /// オートパンの深さ [0.0～1.0]。1.0 にすると左右が最大限に振れ、0.0 でオートパンなし。
    /// デフォルト: 0.3
    /// </summary>
    public double PanDepth = 0.3;

    /// <summary>
    /// LFO波形の種類。Sine, Triangle, Square。
    /// デフォルト: Sine
    /// </summary>
    public LfoWaveShape WaveShape = LfoWaveShape.Sine;

    /// <summary>
    /// LFOのランダム変動量 [0.0～1.0]。
    /// 大きいほどLFOの周波数や変調がランダムに揺らぎ、予測不能なエフェクトとなる。
    /// デフォルト: 0.2
    /// </summary>
    public double RandomModIntensity = 0.2;

    /// <summary>
    /// LFOの初期位相をランダムにするかどうか。
    /// true であれば、処理開始時のLFO位相が毎回ランダムに変わる。
    /// デフォルト: true
    /// </summary>
    public bool EnableRandomPhase = true;
}

/// <summary>
/// トレモロおよびオートパン(左右振動)エフェクトを付与するクラス。
/// </summary>
public class AiAudioEffect_07_Tremolo : AiAudioEffectBase
{
    /// <summary>
    /// 設定を生成するための基本ファクトリ。実装そのまま。
    /// </summary>
    protected override IAiAudioEffectSettings NewSettingsFactoryImpl()
        => new AiAudioEffect_07_Tremolo_Settings();

    /// <summary>
    /// 効果のバリエーションを増すためのパラメータをランダム生成する。
    /// Heavy/Normal/Light の指定により効果の強さを調整した例示的実装。
    /// </summary>
    protected override IAiAudioEffectSettings NewSettingsFactoryWithRandomImpl(AiAudioEffectSpeedType type)
    {
        var ret = new AiAudioEffect_07_Tremolo_Settings();

        // 例示的に switch-case でパラメータの範囲を変化させる
        // （トレモロ周波数・深さ・オートパン・ランダム変動などを調整）
        switch (type)
        {
            case AiAudioEffectSpeedType.Heavy:
            /*                // 強烈な変化を想定
                            ret.LfoRateHz = Random.Shared.NextDouble() * (12.0 - 4.0) + 4.0;       // 4.0～12.0
                            ret.TremoloDepth = Random.Shared.NextDouble() * (1.0 - 0.7) + 0.7;  // 0.7～1.0
                            ret.PanDepth = Random.Shared.NextDouble() * (1.0 - 0.5) + 0.5;      // 0.5～1.0
                            ret.RandomModIntensity = Random.Shared.NextDouble() * (0.8 - 0.3) + 0.3; // 0.3～0.8
                            ret.WaveShape = (LfoWaveShape)Random.Shared.Next(0, 3); // 0,1,2 のいずれか (Sine/Triangle/Square)
                            ret.EnableRandomPhase = (Random.Shared.NextDouble() < 0.8); // 80%程度で位相ランダム
                            break;
            */
            case AiAudioEffectSpeedType.Normal:
                // 標準的な変化を想定
                ret.LfoRateHz = Random.Shared.NextDouble() * (8.0 - 2.0) + 2.0;      // 2.0～8.0
                ret.TremoloDepth = Random.Shared.NextDouble() * (0.7 - 0.3) + 0.3;  // 0.3～0.7
                ret.PanDepth = Random.Shared.NextDouble() * (0.6 - 0.2) + 0.2;      // 0.2～0.6
                ret.RandomModIntensity = Random.Shared.NextDouble() * (0.5 - 0.1) + 0.1; // 0.1～0.5
                ret.WaveShape = (LfoWaveShape)Random.Shared.Next(0, 3);
                ret.EnableRandomPhase = (Random.Shared.NextDouble() < 0.5); // 50%で位相ランダム
                break;

            case AiAudioEffectSpeedType.Light:
                // 弱い変化を想定
                ret.LfoRateHz = Random.Shared.NextDouble() * (3.0 - 0.5) + 0.5;      // 0.5～3.0
                ret.TremoloDepth = Random.Shared.NextDouble() * (0.4 - 0.05) + 0.05; // 0.05～0.4
                ret.PanDepth = Random.Shared.NextDouble() * (0.3 - 0.0) + 0.0;       // 0.0～0.3
                ret.RandomModIntensity = Random.Shared.NextDouble() * (0.3 - 0.0) + 0.0; // 0.0～0.3
                ret.WaveShape = (LfoWaveShape)Random.Shared.Next(0, 3);
                ret.EnableRandomPhase = (Random.Shared.NextDouble() < 0.3); // 30%で位相ランダム
                break;
        }

        return ret;
    }

    /// <summary>
    /// 実際に WAV データを操作してトレモロ＆オートパンエフェクトを付与するメイン処理。
    /// </summary>
    protected override void ProcessFilterImpl(Memory<byte> waveFileInOut, IAiAudioEffectSettings effectSettings, CancellationToken cancel)
    {
        // 設定をキャスト
        AiAudioEffect_07_Tremolo_Settings settings = (AiAudioEffect_07_Tremolo_Settings)effectSettings;

        // 44.1 kHz, 16bit, ステレオ(2ch)を想定
        const int bytesPerSample = 2;   // 16bit = 2 bytes
        const int channels = 2;         // ステレオ
        const int sampleRate = 44100;   // 44.1 kHz

        // 波形データ部分(PCM)を操作するためにSpanを取得
        Span<byte> audioSpan = waveFileInOut.Span;

        // 全サンプルフレーム数（1フレーム = 左右2chぶんのサンプル）
        int frameCount = audioSpan.Length / (bytesPerSample * channels);

        // LFOの初期位相を設定
        double phase = 0.0;
        if (settings.EnableRandomPhase)
        {
            phase = Random.Shared.NextDouble() * Math.PI * 2.0;
        }

        // LFO位相の増分 (基本周波数に相当)
        // 後に、ランダムドリフトを加味して変動させる
        double basePhaseIncrement = 2.0 * Math.PI * settings.LfoRateHz / sampleRate;

        // ランダムドリフトを適用する区間(秒)とそのサンプル数
        // 例: 0.25秒ごとにLFO周波数を少し変動させる
        double driftCycleSec = 0.25;
        int driftCycleSamples = (int)(driftCycleSec * sampleRate);

        // 現在のランダムドリフト値 (周波数追加分, Hz)
        double currentDriftFreq = 0.0;
        // 周波数ドリフトを位相インクリメントに変換した値
        double currentDriftIncrement = 0.0;

        // LFO波形生成用関数
        // waveShapeに応じて [-1, +1] の範囲の値を返す
        double LfoWaveValue(double ph, LfoWaveShape shape)
        {
            switch (shape)
            {
                case LfoWaveShape.Sine:
                    return Math.Sin(ph);
                case LfoWaveShape.Triangle:
                    {
                        // 三角波: 位相を [0, 2π) -> [-1,1] に変換
                        // 一般的には|2*(ph/(2π) - floor(0.5 + ph/(2π)))|*2-1 等で表せるが、
                        // ここでは簡易的に実装
                        // 0～π で上昇, π～2π で下降
                        double t = (ph % (2.0 * Math.PI)) / (Math.PI);
                        // t: 0～2
                        if (t < 1.0)
                        {
                            // 0～1: 上昇
                            return (t * 2.0) - 1.0; // -1～+1
                        }
                        else
                        {
                            // 1～2: 下降
                            return 1.0 - ((t - 1.0) * 2.0); // +1～-1
                        }
                    }
                case LfoWaveShape.Square:
                    return (Math.Sin(ph) >= 0.0) ? 1.0 : -1.0;
                default:
                    return Math.Sin(ph);
            }
        }

        // 周波数ランダムドリフトを更新するローカル関数
        void UpdateRandomDrift()
        {
            // settings.RandomModIntensity = 0.2 なら、 周波数(LfoRateHz)の ±20% 程度をランダムで上下させるようなイメージ
            double maxDriftHz = settings.LfoRateHz * settings.RandomModIntensity;
            currentDriftFreq = (Random.Shared.NextDouble() * 2.0 - 1.0) * maxDriftHz;
            // basePhaseIncrement + drift の合計をサンプルレートで割った値に
            currentDriftIncrement = 2.0 * Math.PI * (settings.LfoRateHz + currentDriftFreq) / sampleRate;
        }

        // RandomModIntensity > 0 であればランダムドリフトを利用
        bool useRandomDrift = (settings.RandomModIntensity > 0.0);

        int nextDriftUpdate = 0;
        if (useRandomDrift)
        {
            UpdateRandomDrift();
            nextDriftUpdate = driftCycleSamples;
        }
        else
        {
            // ランダムドリフトを使わない場合
            currentDriftIncrement = basePhaseIncrement;
        }

        // 実処理ループ
        for (int i = 0; i < frameCount; i++)
        {
            // 中断要求があれば例外をスロー (外側でキャッチする想定)
            if ((i % 2048) == 0) // 適当な頻度でチェック
            {
                cancel.ThrowIfCancellationRequested();
            }

            // LFO計算
            double lfoVal = LfoWaveValue(phase, settings.WaveShape); // [-1, +1]

            // トレモロ計算
            // (1 - depth) + depth * (lfoValの正規化)
            // lfoVal: -1～+1 -> 0～+1 にシフトするには (lfoVal + 1.0)/2
            double amplitude = (1.0 - settings.TremoloDepth)
                               + settings.TremoloDepth * ((lfoVal + 1.0) * 0.5);
            // ただし 0～1.0 を想定

            // オートパン計算
            // lfoVal: -1～+1 をそのままパン変化に使う場合、PanDepthを乗じる
            // 左右のボリューム比率が [ (1 - pan), (1 + pan) ] になるように
            // pan は [-PanDepth, +PanDepth]
            double pan = lfoVal * settings.PanDepth; // [-PanDepth, +PanDepth]

            // 左右それぞれの最終ゲインを計算
            double leftGain = amplitude * (1.0 - 0.5 * pan);  // pan>0 なら右を大きく、左を小さく
            double rightGain = amplitude * (1.0 + 0.5 * pan); // pan<0 なら左を大きく、右を小さく

            // PCMデータを読み取り (16bit, little-endian)
            int idx = i * channels * bytesPerSample;
            short leftSample = (short)(audioSpan[idx] | (audioSpan[idx + 1] << 8));
            short rightSample = (short)(audioSpan[idx + 2] | (audioSpan[idx + 3] << 8));

            // floatに変換してゲインをかける
            double leftFloat = leftSample / 32768.0;
            double rightFloat = rightSample / 32768.0;

            leftFloat *= leftGain;
            rightFloat *= rightGain;

            // クリッピングしないように -1.0～+1.0 の範囲に収めてshortに変換
            int newLeft = (int)(leftFloat * 32767.0);
            int newRight = (int)(rightFloat * 32767.0);

            if (newLeft > short.MaxValue) newLeft = short.MaxValue;
            if (newLeft < short.MinValue) newLeft = short.MinValue;
            if (newRight > short.MaxValue) newRight = short.MaxValue;
            if (newRight < short.MinValue) newRight = short.MinValue;

            // メモリに書き戻し
            audioSpan[idx] = (byte)(newLeft & 0xFF);
            audioSpan[idx + 1] = (byte)((newLeft >> 8) & 0xFF);
            audioSpan[idx + 2] = (byte)(newRight & 0xFF);
            audioSpan[idx + 3] = (byte)((newRight >> 8) & 0xFF);

            // 次のサンプル向けに位相を進める (ドリフト含め)
            phase += currentDriftIncrement;

            // ランダムドリフトの更新タイミング
            if (useRandomDrift)
            {
                if (i == nextDriftUpdate)
                {
                    UpdateRandomDrift();
                    nextDriftUpdate += driftCycleSamples;
                }
            }
        }
    }
}

/// <summary>
/// LFOの波形を表す列挙型。
/// </summary>
[Flags]
public enum LfoWaveShape
{
    Sine = 0,
    Triangle = 1,
    Square = 2
}

#endregion



#region AiAudioEffect_09_Flanger
// このクラスが、処理の内容 (効果の程度や挙動の変化) を決定する枢要なパラメータ指定部分である。
public class AiAudioEffect_09_Flanger_Settings : IAiAudioEffectSettings
{
    /*
     * フランジャーエフェクト用のパラメータ例。すべてのパラメータに、
     * 標準的な効果が得られる値をデフォルト値として代入する。
     */

    /// <summary>
    /// フランジャーの基本ディレイ時間 (ミリ秒)。
    /// 値が大きいほど、原音に対する遅延音のずれが大きくなる。
    /// 一般的には 1～5 ms 程度が多い。
    /// ここでは標準的な 2 ms をデフォルト値とする。
    /// </summary>
    public double BaseDelayMs = 2.0;

    /// <summary>
    /// ディレイ変調の深さ (ミリ秒)。
    /// LFO によるディレイ時間変化の最大幅。
    /// 値が大きいほど、うねりが強くなる。
    /// ここでは標準的な 2 ms をデフォルト値とする。
    /// </summary>
    public double DepthMs = 2.0;

    /// <summary>
    /// 変調周波数 (Hz)。
    /// フランジャーが 1 秒間に何回うねりのサイクルを作るか。
    /// 値が大きいと LFO が速く、うねりの速度が速い。
    /// ここでは標準的な 0.5 Hz をデフォルト値とする。
    /// </summary>
    public double RateHz = 0.5;

    /// <summary>
    /// フィードバック率 (0.0～0.95 程度)。
    /// 遅延音をどの程度リングバッファに再帰させるか。
    /// 値が大きいほど効果が強調され、深いうなりが得られるが、
    /// ホウリングに近づくこともある。
    /// ここでは 0.3 をデフォルト値とする。
    /// </summary>
    public double Feedback = 0.3;

    /// <summary>
    /// エフェクトのウェット成分とドライ成分のミックス比 (0.0～1.0)。
    /// 0 なら原音のみ、1 ならエフェクト音のみ。
    /// ここでは 0.5 をデフォルト値とし、半々にミックスする。
    /// </summary>
    public double Mix = 0.5;

    /// <summary>
    /// ランダム変調を追加的に行うかのフラグ。
    /// true にすると、LFO 周波数やディレイ時間を
    /// ランダムに揺らす動作が加わり、予測不能感を高める。
    /// ここでは false をデフォルト値とする。
    /// </summary>
    public bool RandomizeLFO = false;
}

// このクラスが、処理の内容を実装するクラスである。
public class AiAudioEffect_09_Flanger : AiAudioEffectBase
{
    // この関数は、そのまま、書き写すこと。
    protected override IAiAudioEffectSettings NewSettingsFactoryImpl() => new AiAudioEffect_09_Flanger_Settings();

    // 効果のバリエーションを増すためのパラメータのランダム設定を、
    // おおまかな、「強・中・弱」の効果の強さの程度の指定をある程度もとにして行う関数。
    protected override IAiAudioEffectSettings NewSettingsFactoryWithRandomImpl(AiAudioEffectSpeedType type)
    {
        var ret = new AiAudioEffect_09_Flanger_Settings();

        /* 実装部分ここから */
        switch (type)
        {
            case AiAudioEffectSpeedType.Heavy:
                // 「強い」フランジャー効果を狙う: 深い遅延、速めまたは中程度の LFO、フィードバックも強め
                ret.BaseDelayMs = Random.Shared.NextDouble() * (5.0 - 2.0) + 2.0;      // 2.0 ～ 5.0 ms
                ret.DepthMs = Random.Shared.NextDouble() * (4.0 - 2.0) + 2.0;      // 2.0 ～ 4.0 ms
                ret.RateHz = Random.Shared.NextDouble() * (2.0 - 0.2) + 0.2;      // 0.2 ～ 2.0 Hz
                ret.Feedback = Random.Shared.NextDouble() * (0.9 - 0.5) + 0.5;      // 0.5 ～ 0.9
                ret.Mix = Random.Shared.NextDouble() * (1.0 - 0.7) + 0.7;      // 0.7 ～ 1.0
                ret.RandomizeLFO = (Random.Shared.NextDouble() < 0.8);                 // 80% でランダム変調を付加
                break;

            case AiAudioEffectSpeedType.Normal:
                // 「中程度」フランジャー効果: 適度な遅延と深さ、フィードバックは中程度
                ret.BaseDelayMs = Random.Shared.NextDouble() * (3.0 - 1.0) + 1.0;      // 1.0 ～ 3.0 ms
                ret.DepthMs = Random.Shared.NextDouble() * (3.0 - 1.0) + 1.0;      // 1.0 ～ 3.0 ms
                ret.RateHz = Random.Shared.NextDouble() * (1.0 - 0.2) + 0.2;      // 0.2 ～ 1.0 Hz
                ret.Feedback = Random.Shared.NextDouble() * (0.5 - 0.2) + 0.2;      // 0.2 ～ 0.5
                ret.Mix = Random.Shared.NextDouble() * (0.7 - 0.3) + 0.3;      // 0.3 ～ 0.7
                ret.RandomizeLFO = (Random.Shared.NextDouble() < 0.5);                 // 50% でランダム変調を付加
                break;

            case AiAudioEffectSpeedType.Light:
                // 「弱い」フランジャー効果: 小さい遅延、浅めの深さ、穏やかな変調
                ret.BaseDelayMs = Random.Shared.NextDouble() * (2.0 - 0.5) + 0.5;      // 0.5 ～ 2.0 ms
                ret.DepthMs = Random.Shared.NextDouble() * (2.0 - 0.5) + 0.5;      // 0.5 ～ 2.0 ms
                ret.RateHz = Random.Shared.NextDouble() * (0.5 - 0.05) + 0.05;    // 0.05 ～ 0.5 Hz
                ret.Feedback = Random.Shared.NextDouble() * (0.3 - 0.0) + 0.0;      // 0.0 ～ 0.3
                ret.Mix = Random.Shared.NextDouble() * (0.5 - 0.2) + 0.2;      // 0.2 ～ 0.5
                ret.RandomizeLFO = (Random.Shared.NextDouble() < 0.3);                 // 30% でランダム変調を付加
                break;
        }
        /* 実装部分ここまで */

        return ret;
    }

    // この関数が、「効果」を生み出す処理を実際に行なう枢要部分である。
    protected override void ProcessFilterImpl(Memory<byte> waveFileInOut, IAiAudioEffectSettings effectSettings, CancellationToken cancel)
    {
        // まず、effectSettings の内容を、扱いやすいように、AiAudioEffect_09_Flanger_Settings にキャストする。
        AiAudioEffect_09_Flanger_Settings settings = (AiAudioEffect_09_Flanger_Settings)effectSettings;

        /* 実装部分ここから */

        // waveFileInOut は、44.1 kHz / 2チャンネル / 16ビット PCM の波形データ (ヘッダなし) である。
        // 1サンプルあたり2バイト、ステレオなので1フレーム4バイト。

        var span = waveFileInOut.Span;
        int totalBytes = span.Length;
        // ステレオ 16ビット = 4バイト/フレーム
        int totalFrames = totalBytes / 4;
        if (totalFrames == 0) return;

        // フランジャー実装に使用するリングバッファを用意する。
        // 最大でも 10 ms ほどあれば一般的なフランジャーには十分だが、
        // ユーザーがもっと大きい値を指定する場合を考慮して安全に確保する。
        // 例として 50 ms ぶんを確保 (44100Hzで約2205サンプル) にしておく。
        int maxBufferSamples = 44100 / 20; // 50ms → 2205サンプル程度
        if (maxBufferSamples < 1) maxBufferSamples = 1;
        float[] ringBufferL = new float[maxBufferSamples];
        float[] ringBufferR = new float[maxBufferSamples];

        // 書き込み位置
        int writePos = 0;

        // LFO 用のフェーズ
        double lfoPhase = 0.0;

        // サンプリングレート
        const double sampleRate = 44100.0;

        // ランダム変調用のパラメータ
        // RandomizeLFO が true の時に、一定間隔で少しずつ変化させる
        double randomLfoRateOffset = 0.0;    // LFO周波数をランダムにずらす
        double randomDepthOffset = 0.0;    // DepthMsをランダムに揺らす
        int randomLfoCounter = 0;
        // 何フレーム毎にランダム変動を行うか (ここでは 0.1 秒ごと = 4410 フレームごと)
        int randomLfoInterval = 4410;

        // 各フレームを順に処理
        for (int i = 0; i < totalFrames; i++)
        {
            // 中断要求チェック (頻繁にやると遅いので 1024フレームごと等で良い)
            if ((i & 1023) == 0) cancel.ThrowIfCancellationRequested();

            // 入力波形読み取り (16ビット、ステレオ)
            int byteIndex = i * 4;
            short sampleLeft = (short)(span[byteIndex + 0] | (span[byteIndex + 1] << 8));
            short sampleRight = (short)(span[byteIndex + 2] | (span[byteIndex + 3] << 8));

            // float に変換(-1.0～1.0)
            float inL = sampleLeft / 32768.0f;
            float inR = sampleRight / 32768.0f;

            // LFO変調周波数とDepthにランダム揺らぎを付与 (RandomizeLFO == true の場合)
            if (settings.RandomizeLFO && randomLfoCounter++ >= randomLfoInterval)
            {
                randomLfoCounter = 0;
                // 周波数を ±(0.1Hz 程度) で揺らす
                double rnd1 = (Random.Shared.NextDouble() - 0.5) * 0.2;
                // Depth を ±(1.0ms 程度) で揺らす
                double rnd2 = (Random.Shared.NextDouble() - 0.5) * 2.0;

                // 反映 (過度にパラメータがおかしくならないよう軽減して加算)
                randomLfoRateOffset += rnd1;
                randomDepthOffset += rnd2;

                // クリップ (負の周波数や極端な数値にならないよう制限)
                if (settings.RateHz + randomLfoRateOffset < 0.01) randomLfoRateOffset = 0.01 - settings.RateHz;
                if (settings.RateHz + randomLfoRateOffset > 5.0) randomLfoRateOffset = 5.0 - settings.RateHz;
                if (settings.DepthMs + randomDepthOffset < 0.1) randomDepthOffset = 0.1 - settings.DepthMs;
                if (settings.DepthMs + randomDepthOffset > 20.0) randomDepthOffset = 20.0 - settings.DepthMs;
            }

            // 現在の LFO 周波数
            double currentRateHz = settings.RateHz + (settings.RandomizeLFO ? randomLfoRateOffset : 0.0);
            // 現在の変調深さ (ms)
            double currentDepthMs = settings.DepthMs + (settings.RandomizeLFO ? randomDepthOffset : 0.0);

            // LFO位相の計算
            double lfoValue = Math.Sin(2.0 * Math.PI * lfoPhase);

            // 次のサンプルに進める前に LFO位相を更新
            // 1フレームあたりの位相増分 = (RateHz / sampleRate)
            lfoPhase += currentRateHz / sampleRate;
            // LFO位相を 0～1 の範囲でループさせる
            if (lfoPhase >= 1.0) lfoPhase -= 1.0;

            // 実効遅延時間 (ms) = BaseDelayMs + (LFOによる変調)
            // LFO を -1～+1 で変調させるため、DepthMs * 0.5 * lfoValue
            double delayMs = settings.BaseDelayMs + (currentDepthMs * 0.5 * lfoValue);

            // サンプル数に変換
            double delaySamples = (delayMs * sampleRate) / 1000.0;

            // リングバッファ読み出し位置 (writePos - 遅延サンプル)
            // フラクショナル遅延を実現するために補間を行う
            double readPos = (double)writePos - delaySamples;
            // リングバッファ範囲内に収める (モジュロ演算)
            // 負になったら末尾へ回す
            while (readPos < 0.0)
            {
                readPos += maxBufferSamples;
            }
            while (readPos >= maxBufferSamples)
            {
                readPos -= maxBufferSamples;
            }

            // 補間用にフロアと小数部を求める
            int basePos = (int)readPos;
            double frac = readPos - basePos;
            int basePos1 = basePos + 1;
            if (basePos1 >= maxBufferSamples) basePos1 -= maxBufferSamples;

            // リングバッファから遅延サンプルを取り出す (線形補間)
            float dL0 = ringBufferL[basePos];
            float dL1 = ringBufferL[basePos1];
            float delayedL = (float)(dL0 * (1.0 - frac) + dL1 * frac);

            float dR0 = ringBufferR[basePos];
            float dR1 = ringBufferR[basePos1];
            float delayedR = (float)(dR0 * (1.0 - frac) + dR1 * frac);

            // 出力サンプル (Wet / Dry ミックス)
            float outL = inL * (float)(1.0 - settings.Mix) + delayedL * (float)settings.Mix;
            float outR = inR * (float)(1.0 - settings.Mix) + delayedR * (float)settings.Mix;

            // リングバッファへの書き込み (フィードバックも加味)
            float fbL = inL + delayedL * (float)settings.Feedback;
            float fbR = inR + delayedR * (float)settings.Feedback;

            ringBufferL[writePos] = fbL;
            ringBufferR[writePos] = fbR;

            // 書き込み位置を進める
            writePos++;
            if (writePos >= maxBufferSamples) writePos = 0;

            // 出力を short に戻してメモリへ書き込む (クリッピングに注意)
            short outSampleL = (short)Math.Clamp((int)(outL * 32767.0f), short.MinValue, short.MaxValue);
            short outSampleR = (short)Math.Clamp((int)(outR * 32767.0f), short.MinValue, short.MaxValue);

            span[byteIndex + 0] = (byte)(outSampleL & 0xFF);
            span[byteIndex + 1] = (byte)((outSampleL >> 8) & 0xFF);
            span[byteIndex + 2] = (byte)(outSampleR & 0xFF);
            span[byteIndex + 3] = (byte)((outSampleR >> 8) & 0xFF);
        }

        /* 実装部分ここまで */
    }
}
#endregion



#region AiAudioEffect_12_DelayEcho
// このクラスが、処理の内容 (効果の程度や挙動の変化) を決定する枢要なパラメータ指定部分である。
public class AiAudioEffect_12_DelayEcho_Settings : IAiAudioEffectSettings
{
    /* 実装部分ここから
     *
     * ★ ディレイ・エコー効果を調整するためのパラメータを設定。
     *   - DelayTimeMs    : 遅延時間（ミリ秒）
     *   - Feedback       : 反復回数を決めるフィードバック量（0.0～1.0）
     *   - DryMix         : 原音(ドライ音)の音量比率（0.0～1.0）
     *   - WetMix         : エコー音(ウェット音)の音量比率（0.0～1.0）
     *   - PingPong       : ピンポン・ディレイを行うかどうかのフラグ
     *
     * ※ デフォルト値は、基本的なエコー効果が聴こえるよう設定。
     */

    /// <summary>
    /// 遅延時間（ミリ秒）。たとえば 300ms 程度にするとほどよい残響。
    /// 短すぎる（50ms 以下）とスラップバックに近く、200～400ms 程度で一般的なエコー感、
    /// 500ms 以上にすると空間がかなり広く感じられる。
    /// </summary>
    public double DelayTimeMs = 300.0;

    /// <summary>
    /// フィードバック量（0.0～1.0）。0.0 なら1回だけ遅延音が鳴り、
    /// 1.0 に近いほどディレイ音が何度も反復して長く残る。
    /// </summary>
    public double Feedback = 0.5;

    /// <summary>
    /// 原音(ドライ音)の音量比率（0.0～1.0）。0.0 にすると完全にエコー音だけ、
    /// 1.0 に近いほど元の音声の存在感が大きい。
    /// </summary>
    public double DryMix = 0.8;

    /// <summary>
    /// エコー音(ウェット音)の音量比率（0.0～1.0）。0.0 にするとエコーが聞こえず、
    /// 1.0 に近いほどエコー音が大きくなる。
    /// </summary>
    public double WetMix = 0.4;

    /// <summary>
    /// ピンポン・ディレイを有効にするかどうかのフラグ。
    /// true にすると左右のチャンネルが交互にエコー音を反復し、不思議な広がりが得られる。
    /// </summary>
    public bool PingPong = false;

    /* 実装部分ここまで */
}

// このクラスが、処理の内容を実装するクラスである。
public class AiAudioEffect_12_DelayEcho : AiAudioEffectBase
{
    // この関数は、そのまま、書き写すこと。
    protected override IAiAudioEffectSettings NewSettingsFactoryImpl() => new AiAudioEffect_12_DelayEcho_Settings();

    // 効果のバリエーションを増すためのパラメータのランダム設定を、おおまかな、「強・注・弱」の効果の強さの程度の指定をある程度もとにして、助ける関数
    protected override IAiAudioEffectSettings NewSettingsFactoryWithRandomImpl(AiAudioEffectSpeedType type)
    {
        var ret = new AiAudioEffect_12_DelayEcho_Settings();

        /* 実装部分ここから
         *
         * type の値に従って、ランダムパラメータを設定し、
         * Heavy -> 強めのエコー（長めのディレイ、高めのフィードバック）、
         * Normal -> 中程度、
         * Light -> 短めのディレイ、フィードバックも控えめ、
         * といった範囲内で乱数を使って設定する例。
         */

        switch (type)
        {
            case AiAudioEffectSpeedType.Heavy:
                // ディレイタイム 400～800ms
                ret.DelayTimeMs = Random.Shared.NextDouble() * (800.0 - 400.0) + 400.0;
                // フィードバック量 0.6～0.9
                ret.Feedback = Random.Shared.NextDouble() * (0.9 - 0.6) + 0.6;
                // ドライ音量 0.5～0.8
                ret.DryMix = Random.Shared.NextDouble() * (0.8 - 0.5) + 0.5;
                // ウェット音量 0.6～1.0
                ret.WetMix = Random.Shared.NextDouble() * (1.0 - 0.6) + 0.6;
                // ピンポン確率 50%
                ret.PingPong = Random.Shared.NextDouble() < 0.5;
                break;

            case AiAudioEffectSpeedType.Normal:
                // ディレイタイム 200～400ms
                ret.DelayTimeMs = Random.Shared.NextDouble() * (400.0 - 200.0) + 200.0;
                // フィードバック量 0.3～0.6
                ret.Feedback = Random.Shared.NextDouble() * (0.6 - 0.3) + 0.3;
                // ドライ音量 0.7～0.9
                ret.DryMix = Random.Shared.NextDouble() * (0.9 - 0.7) + 0.7;
                // ウェット音量 0.3～0.6
                ret.WetMix = Random.Shared.NextDouble() * (0.6 - 0.3) + 0.3;
                // ピンポン確率 30%
                ret.PingPong = Random.Shared.NextDouble() < 0.3;
                break;

            case AiAudioEffectSpeedType.Light:
                // ディレイタイム 80～200ms
                ret.DelayTimeMs = Random.Shared.NextDouble() * (200.0 - 80.0) + 80.0;
                // フィードバック量 0.1～0.3
                ret.Feedback = Random.Shared.NextDouble() * (0.3 - 0.1) + 0.1;
                // ドライ音量 0.9～1.0
                ret.DryMix = Random.Shared.NextDouble() * (1.0 - 0.9) + 0.9;
                // ウェット音量 0.1～0.3
                ret.WetMix = Random.Shared.NextDouble() * (0.3 - 0.1) + 0.1;
                // ピンポン確率 10%
                ret.PingPong = Random.Shared.NextDouble() < 0.1;
                break;
        }

        /* 実装部分ここまで */

        return ret;
    }

    // この関数が、「効果」を生み出す処理を実際に行なう枢要部分である。
    protected override void ProcessFilterImpl(Memory<byte> waveFileInOut, IAiAudioEffectSettings effectSettings, CancellationToken cancel)
    {
        // まず、effectSettings の内容を、扱いやすいように、AiAudioEffect_12_DelayEcho_Settings にキャストする。
        AiAudioEffect_12_DelayEcho_Settings settings = (AiAudioEffect_12_DelayEcho_Settings)effectSettings;

        /* 実装部分ここから */

        // ---- コードの枢要をここに実装してください。----

        // 44.1 kHz / 16ビット / ステレオの WAV データであることを前提にする。
        // waveFileInOut のサイズ (バイト数) / 4 = ステレオのサンプルフレーム数 となる。
        // (左チャンネル 2バイト + 右チャンネル 2バイト = 4バイトで1フレーム)
        int totalBytes = waveFileInOut.Length;
        int totalFrames = totalBytes / 4; // ステレオのフレーム数

        // 16ビット整数(short)配列へ変換
        // （NAudio.Waveなどでラップせず、直接バイナリ操作するため）
        short[] sampleData = new short[totalBytes / 2];
        Buffer.BlockCopy(waveFileInOut.ToArray(), 0, sampleData, 0, totalBytes);

        // ディレイタイム（サンプル単位）
        // たとえば 300ms なら、44100 * 0.300 = 13230 サンプル分ほどの遅延となる。
        int delaySamples = (int)(44100.0 * (settings.DelayTimeMs / 1000.0));

        // 安全のため、最低1サンプル以上確保
        if (delaySamples < 1)
        {
            delaySamples = 1;
        }

        // ピンポンディレイなどで最大 2秒 ほどを想定したバッファを確保
        // （必要に応じて拡大しても良い）
        int maxDelaySamples = 44100 * 2;
        // 左右それぞれにリングバッファを用意
        float[] delayBufferLeft = new float[maxDelaySamples];
        float[] delayBufferRight = new float[maxDelaySamples];

        // リングバッファ読み書きの開始位置
        // writePos を 0、readPos を (0 - delaySamples) mod maxDelaySamples などで開始すると
        // 最初のほうの読出しは0が返ってくる形になる。
        int writePos = 0;
        int readPos = (maxDelaySamples - delaySamples) % maxDelaySamples;

        // パラメータの取得
        double feedback = settings.Feedback;  // 0.0～1.0
        double dryMix = settings.DryMix;       // 0.0～1.0
        double wetMix = settings.WetMix;       // 0.0～1.0
        bool pingPong = settings.PingPong;

        // ★ ここではランダムな変化を少し与える例として、
        //    一定間隔ごとに feedback や wetMix を微調整する実装を簡易で入れてみる。
        //    （ランダム変化を大きくしたい場合はここを工夫してください）
        Random rnd = new Random();
        int nextParamChangeFrame = 0; // 次にパラメータを変化させるサンプルフレーム

        for (int i = 0; i < totalFrames; i++)
        {
            // 一定周期でキャンセルをチェック（大きなループのとき）
            if ((i & 0xFFF) == 0) // 4096フレームごと
            {
                cancel.ThrowIfCancellationRequested();
            }

            // 左右のサンプルを short -> float(-1.0～1.0) に変換
            short leftS = sampleData[i * 2 + 0];
            short rightS = sampleData[i * 2 + 1];
            float left = leftS / 32768.0f;
            float right = rightS / 32768.0f;

            // リングバッファからディレイ音を取得
            float delayedLeft = delayBufferLeft[readPos];
            float delayedRight = delayBufferRight[readPos];

            // 出力サンプルを計算
            float outLeft;
            float outRight;

            if (!pingPong)
            {
                // 通常のステレオディレイ：LはLを遅延、RはRを遅延
                outLeft = (float)(left * dryMix) + delayedLeft * (float)wetMix;
                outRight = (float)(right * dryMix) + delayedRight * (float)wetMix;

                // 次のエコーのためにリングバッファに書き込み
                delayBufferLeft[writePos] = outLeft * (float)feedback;
                delayBufferRight[writePos] = outRight * (float)feedback;
            }
            else
            {
                // ピンポンディレイ：L ← Rの遅延音、R ← Lの遅延音 を混合
                outLeft = (float)(left * dryMix) + delayedRight * (float)wetMix;
                outRight = (float)(right * dryMix) + delayedLeft * (float)wetMix;

                // 次のエコーのためにリングバッファに書き込み（左右逆にフィードバック）
                delayBufferLeft[writePos] = outRight * (float)feedback;
                delayBufferRight[writePos] = outLeft * (float)feedback;
            }

            // 計算結果を short に戻す前にクリッピング
            short outLeftS = (short)Math.Clamp(outLeft * 32767.0f, -32768, 32767);
            short outRightS = (short)Math.Clamp(outRight * 32767.0f, -32768, 32767);

            // 計算結果を書き戻し（In/Outバッファ）
            sampleData[i * 2 + 0] = outLeftS;
            sampleData[i * 2 + 1] = outRightS;

            // リングバッファの読み書き位置を進める
            readPos = (readPos + 1) % maxDelaySamples;
            writePos = (writePos + 1) % maxDelaySamples;

            // ★ ランダム変化例：ある程度のフレーム間隔で feedback, wetMix を小幅に揺らす
            if (i == nextParamChangeFrame)
            {
                // ±0.05 の範囲で微調整（0.0～1.0 にクリップ）
                double fbVariation = feedback + (rnd.NextDouble() * 0.1 - 0.05);
                double wmVariation = wetMix + (rnd.NextDouble() * 0.1 - 0.05);
                feedback = Math.Min(Math.Max(fbVariation, 0.0), 1.0);
                wetMix = Math.Min(Math.Max(wmVariation, 0.0), 1.0);

                // 次の変化タイミングを 20000～40000 フレーム後（約0.45秒～0.90秒後）にセット
                // （44100Hz換算で）
                int interval = rnd.Next(20000, 40000);
                nextParamChangeFrame = i + interval;
            }
        }

        // 処理後のデータを waveFileInOut に書き戻す
        sampleData.CopyTo(waveFileInOut.Span._AsSInt16Span());

        /* 実装部分ここまで */
    }
}
#endregion





#region AiAudioEffect_15_SidechainPumping
// このクラスが、処理の内容 (効果の程度や挙動の変化) を決定する枢要なパラメータ指定部分である。
public class AiAudioEffect_15_SidechainPumping_Settings : IAiAudioEffectSettings
{
    /// <summary>
    /// ポンピングの深さを指定する。0.0 なら音量ディップなし（効果オフ相当）、1.0 なら最大ディップ。
    /// デフォルト値 0.7 は比較的はっきりとしたポンピングが得られる標準的な設定。
    /// </summary>
    public double PumpDepth = 0.7;

    /// <summary>
    /// 1秒あたりのポンピング回数。例えば 2.0 なら1秒に2回のディップ（= 2Hz = 1分間に120BPMの4拍とみなせる）。
    /// デフォルト値 2.0 はやや速いダンスビート的なポンピング。
    /// </summary>
    public double PumpFrequency = 2.0;

    /// <summary>
    /// アタックタイム (ms)。ディップ開始時、音量が最大から最小へ下がるのに要する時間。
    /// デフォルト 10.0ms は比較的早い立ち上がりで、明瞭なディップを生む。
    /// </summary>
    public double AttackTimeMs = 10.0;

    /// <summary>
    /// リリースタイム (ms)。ディップの後、音量が最小から元に戻るまでの時間。
    /// デフォルト 300.0ms はゆっくりとした回復で、EDM系にありがちな息をするようなうねりが得られる。
    /// </summary>
    public double ReleaseTimeMs = 300.0;

    /// <summary>
    /// ポンピング周波数や深さをどの程度ランダムに変動させるかを示す強度。
    /// 0.0 なら変動なし、1.0 なら大きく変化する。デフォルト 0.2。
    /// </summary>
    public double VariationIntensity = 0.2;

    /// <summary>
    /// 何秒ごとにランダム変動を行うかの間隔を指定する (秒)。
    /// デフォルト 1.0 は1秒に1回程度変動が入り、程よい予測不能感を得られる。
    /// </summary>
    public double VariationSpeed = 1.0;

    /// <summary>
    /// ランダム変動を行うかどうかのフラグ。true なら行う。false なら行わない。
    /// デフォルト false。
    /// </summary>
    public bool UseRandomVariation = false;
}

// このクラスが、処理の内容を実装するクラスである。
public class AiAudioEffect_15_SidechainPumping : AiAudioEffectBase
{
    // この関数は、そのまま、書き写すこと。
    protected override IAiAudioEffectSettings NewSettingsFactoryImpl() => new AiAudioEffect_15_SidechainPumping_Settings();

    // 効果のバリエーションを増すためのパラメータのランダム設定を、おおまかな、「強・注・弱」の効果の強さの程度の指定をある程度もとにして、助ける関数
    protected override IAiAudioEffectSettings NewSettingsFactoryWithRandomImpl(AiAudioEffectSpeedType type)
    {
        var ret = new AiAudioEffect_15_SidechainPumping_Settings();

        /* 実装部分ここから */
        switch (type)
        {
            case AiAudioEffectSpeedType.Heavy:
                // Deepかつ速いポンピングを想定
                ret.PumpDepth = Random.Shared.NextDouble() * (1.0 - 0.6) + 0.6;    // 0.6～1.0
                ret.PumpFrequency = Random.Shared.NextDouble() * (4.0 - 1.5) + 1.5;  // 1.5～4.0 (1秒あたり1.5～4回のディップ)
                ret.AttackTimeMs = Random.Shared.NextDouble() * (30.0 - 5.0) + 5.0; // 5～30ms
                ret.ReleaseTimeMs = Random.Shared.NextDouble() * (400.0 - 100.0) + 100.0; // 100～400ms
                ret.VariationIntensity = Random.Shared.NextDouble() * (0.5 - 0.2) + 0.2;  // 0.2～0.5
                ret.VariationSpeed = Random.Shared.NextDouble() * (1.0 - 0.3) + 0.3;  // 0.3～1.0秒ごとに変動
                // だいたい 70% くらいの確率でランダム変動を有効化
                ret.UseRandomVariation = (Random.Shared.NextDouble() < 0.7);
                break;

            case AiAudioEffectSpeedType.Normal:
                // 中程度のポンピング
                ret.PumpDepth = Random.Shared.NextDouble() * (0.8 - 0.4) + 0.4;   // 0.4～0.8
                ret.PumpFrequency = Random.Shared.NextDouble() * (3.0 - 1.0) + 1.0;  // 1.0～3.0 (1秒あたり1～3回のディップ)
                ret.AttackTimeMs = Random.Shared.NextDouble() * (50.0 - 10.0) + 10.0; // 10～50ms
                ret.ReleaseTimeMs = Random.Shared.NextDouble() * (600.0 - 200.0) + 200.0; // 200～600ms
                ret.VariationIntensity = Random.Shared.NextDouble() * (0.3 - 0.1) + 0.1;  // 0.1～0.3
                ret.VariationSpeed = Random.Shared.NextDouble() * (2.0 - 1.0) + 1.0;  // 1.0～2.0秒ごとに変動
                // だいたい 50% くらいの確率でランダム変動を有効化
                ret.UseRandomVariation = (Random.Shared.NextDouble() < 0.5);
                break;

            case AiAudioEffectSpeedType.Light:
                // ゆるめ・控えめのポンピング
                ret.PumpDepth = Random.Shared.NextDouble() * (0.5 - 0.2) + 0.2;  // 0.2～0.5
                ret.PumpFrequency = Random.Shared.NextDouble() * (2.0 - 0.5) + 0.5;  // 0.5～2.0 (1秒あたり0.5～2回)
                ret.AttackTimeMs = Random.Shared.NextDouble() * (80.0 - 20.0) + 20.0; // 20～80ms
                ret.ReleaseTimeMs = Random.Shared.NextDouble() * (800.0 - 300.0) + 300.0; // 300～800ms
                ret.VariationIntensity = Random.Shared.NextDouble() * (0.2 - 0.05) + 0.05; // 0.05～0.2
                ret.VariationSpeed = Random.Shared.NextDouble() * (4.0 - 2.0) + 2.0;  // 2.0～4.0秒ごとに変動
                // だいたい 30% くらいの確率でランダム変動を有効化
                ret.UseRandomVariation = (Random.Shared.NextDouble() < 0.3);
                break;
        }
        /* 実装部分ここまで */

        return ret;
    }

    // この関数が、「効果」を生み出す処理を実際に行なう枢要部分である。
    protected override void ProcessFilterImpl(Memory<byte> waveFileInOut, IAiAudioEffectSettings effectSettings, CancellationToken cancel)
    {
        // まず、effectSettings の内容を、扱いやすいように、AiAudioEffect_15_SidechainPumping_Settings にキャストする。
        AiAudioEffect_15_SidechainPumping_Settings settings = (AiAudioEffect_15_SidechainPumping_Settings)effectSettings;

        /* 実装部分ここから */

        // この実装例では、次のような「擬似サイドチェイン」手法を用いる:
        //   1. waveFileInOut には 44.1kHz / 16bit / ステレオ のPCMデータが入っている。
        //   2. 1サンプル(フレーム)につき 4バイト(左ch 2byte + 右ch 2byte)。
        //   3. 指定されたポンピング周波数 PumpFrequency(Hz) = 1秒あたりのディップ回数。
        //   4. アタック(AttackTimeMs)とリリース(ReleaseTimeMs)でゲインを急激に変化させないよう補間。
        //   5. 一定間隔(VariationSpeed秒ごと)にランダム要素を加味し、PumpFrequencyやPumpDepthを揺らす(UseRandomVariation が true の場合)。
        //   6. ゲインが変化しすぎてクリップノイズが生じないように注意。
        //
        // ポンピングの波形: 1周期を0～1に正規化した位相(phaseInCycle)を用い、
        //   [0        ~ attackFrac)      : 1 から 1 - PumpDepth へ線形に下げる(アタック)
        //   [attackFrac ~ attackFrac+releaseFrac) : 1 - PumpDepth から 1 へ線形に戻す(リリース)
        //   [その他                   ] : 1 (ゲイン最大)
        //
        // attackFrac, releaseFrac は PumpFrequency に応じて調整する。周期は (1.0 / PumpFrequency)秒。
        // ただし attackFrac + releaseFrac が1を超える場合は適宜クリップ（または比率再計算）する。
        //
        // ※ 実際のサイドチェインでは別トラックのキックをトリガーにコンプレッサをかけるが、
        //   ここではトリガーなしのLFO的アプローチでポンピング感を再現する。

        Span<byte> span = waveFileInOut.Span;
        int totalBytes = span.Length;
        // 1サンプル(ステレオ1フレーム)あたり4バイト
        int totalFrames = totalBytes / 4;
        if (totalFrames == 0) return; // 音声データがない場合は処理終了

        int sampleRate = 44100; // 44.1kHz固定を想定

        // 周期 (秒)
        double cycleDuration = (settings.PumpFrequency <= 0.0) ? double.MaxValue : (1.0 / settings.PumpFrequency);
        // Attack と Release を周期(0～1に正規化)で表す
        double attackFrac = 0.0;
        double releaseFrac = 0.0;
        if (cycleDuration < double.MaxValue)
        {
            // attackFrac = AttackTimeMs / (周期秒 * 1000)
            attackFrac = settings.AttackTimeMs / (cycleDuration * 1000.0);
            // releaseFrac = ReleaseTimeMs / (周期秒 * 1000)
            releaseFrac = settings.ReleaseTimeMs / (cycleDuration * 1000.0);

            // アタック＋リリースが1を超えるときは、全体を比率でスケールダウンして1に収める
            double sumAR = attackFrac + releaseFrac;
            if (sumAR > 1.0)
            {
                attackFrac /= sumAR;
                releaseFrac /= sumAR;
            }
        }

        // 現在の位相(0～1の小数でカウントし、超えたら折り返す)
        double phaseInCycle = 0.0;

        // ランダム変動用の次回変動発生時刻(秒)
        double nextVariationTime = (settings.UseRandomVariation) ? settings.VariationSpeed : double.MaxValue;
        // 現在の「実際に使用している」PumpFrequency, PumpDepth
        double currentFrequency = settings.PumpFrequency;
        double currentDepth = settings.PumpDepth;

        // 変動などで周期が変わった場合に再計算する関数
        void RecalcCycle()
        {
            cycleDuration = (currentFrequency <= 0.0) ? double.MaxValue : (1.0 / currentFrequency);
            attackFrac = settings.AttackTimeMs / (cycleDuration * 1000.0);
            releaseFrac = settings.ReleaseTimeMs / (cycleDuration * 1000.0);
            double sumAR = attackFrac + releaseFrac;
            if (sumAR > 1.0)
            {
                attackFrac /= sumAR;
                releaseFrac /= sumAR;
            }
        }

        // メインループ
        double currentTimeSec = 0.0;               // 経過時間(秒)
        double deltaTimeSec = 1.0 / sampleRate;  // 1フレーム(ステレオ)あたりの時間(秒)

        for (int i = 0; i < totalFrames; i++)
        {
            // キャンセル要求が来ていないか一定間隔ごとにチェック（負荷を下げるため適宜）
            if ((i & 0xFFF) == 0) // 4096サンプルごとにチェック
            {
                cancel.ThrowIfCancellationRequested();
            }

            // ポンピング・ゲイン計算
            double envelope = 1.0; // ゲイン(0～1)

            // phaseInCycle: 0～1を周期として移動
            //   0.0 で周期開始点、1.0 で次の周期に折り返し
            if (attackFrac + releaseFrac > 0.0)
            {
                if (phaseInCycle < attackFrac)
                {
                    // アタック段階（音量が 1 から (1 - depth) へ線形に下がる）
                    double r = phaseInCycle / attackFrac;
                    envelope = 1.0 - currentDepth * r;
                }
                else if (phaseInCycle < (attackFrac + releaseFrac))
                {
                    // リリース段階（音量が (1 - depth) から 1 へ線形に上がる）
                    double r = (phaseInCycle - attackFrac) / releaseFrac;
                    envelope = (1.0 - currentDepth) + currentDepth * r;
                }
                else
                {
                    // それ以降は 1.0 で一定（最大音量に復帰した状態）
                    envelope = 1.0;
                }
            }

            // 16bitステレオなので、左チャンネル, 右チャンネルの順
            int byteIndex = i * 4;
            short sampleLeft = BitConverter.ToInt16(span.Slice(byteIndex, 2));
            short sampleRight = BitConverter.ToInt16(span.Slice(byteIndex + 2, 2));

            // -32768～32767 の short を -1.0～+1.0 に変換
            double left = sampleLeft / 32768.0;
            double right = sampleRight / 32768.0;

            // エンベロープを適用
            left *= envelope;
            right *= envelope;

            // short に戻す（クリッピングに注意）
            short outLeft = (short)Math.Clamp((int)(left * 32768.0), -32768, 32767);
            short outRight = (short)Math.Clamp((int)(right * 32768.0), -32768, 32767);

            // 書き戻し
            BitConverter.TryWriteBytes(span.Slice(byteIndex, 2), outLeft);
            BitConverter.TryWriteBytes(span.Slice(byteIndex + 2, 2), outRight);

            // 位相を進める
            if (cycleDuration < double.MaxValue)
            {
                phaseInCycle += deltaTimeSec / cycleDuration;
                if (phaseInCycle >= 1.0) phaseInCycle -= 1.0;
            }

            // 時間を進める
            currentTimeSec += deltaTimeSec;

            // ランダム変動
            if (settings.UseRandomVariation && currentTimeSec >= nextVariationTime)
            {
                // 次回変動を設定
                nextVariationTime += settings.VariationSpeed;

                // VariationIntensity を元に、周波数・深さを少し揺らす
                double freqFactor = 1.0 + (Random.Shared.NextDouble() * 2.0 - 1.0) * settings.VariationIntensity;
                double depthFactor = 1.0 + (Random.Shared.NextDouble() * 2.0 - 1.0) * settings.VariationIntensity;
                // 周波数や深さが極端になりすぎないようクランプする
                currentFrequency = Math.Clamp(currentFrequency * freqFactor, 0.01, 20.0);
                currentDepth = Math.Clamp(currentDepth * depthFactor, 0.0, 1.0);

                // 再計算
                RecalcCycle();
            }
        }

        /* 実装部分ここまで */
    }
}
#endregion






#endif

