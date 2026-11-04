"""Settings for the ONNX diarization engine: parsing, clamping and precedence, without models."""

import pytest

from sidecar.src.diarization.onnx_engine import DiarizationSettings


def test_shipped_defaults_are_the_documented_ones():
    settings = DiarizationSettings.from_env_and_params()
    assert settings.threshold == 0.5
    assert settings.num_speakers == -1
    assert settings.min_duration_on == 0.3
    assert settings.min_duration_off == 0.5
    assert settings.merge_gap == 0.0
    assert settings.min_speaker_seconds == 0.0
    assert settings.merge_similarity == 0.5
    assert DiarizationSettings().as_dict()["threshold"] == 0.5


def test_environment_variables_are_read_and_parsed():
    settings = DiarizationSettings.from_env_and_params(
        {},
        {
            "LOCUS_DIARIZATION_THRESHOLD": "0.7",
            "LOCUS_DIARIZATION_NUM_SPEAKERS": "2",
            "LOCUS_DIARIZATION_MIN_ON": "0.6",
            "LOCUS_DIARIZATION_MIN_OFF": "0.8",
        },
    )
    assert (settings.threshold, settings.num_speakers) == (0.7, 2)
    assert (settings.min_duration_on, settings.min_duration_off) == (0.6, 0.8)


def test_rpc_params_override_the_environment():
    settings = DiarizationSettings.from_env_and_params(
        {"threshold": 0.9, "num_speakers": 3},
        {"LOCUS_DIARIZATION_THRESHOLD": "0.7", "LOCUS_DIARIZATION_NUM_SPEAKERS": "2"},
    )
    assert settings.threshold == 0.9
    assert settings.num_speakers == 3


def test_out_of_range_values_are_clamped_instead_of_failing_the_step():
    assert DiarizationSettings.from_env_and_params({"threshold": 9.0}).threshold == 1.5
    assert DiarizationSettings.from_env_and_params({"threshold": 0.0}).threshold == 0.1
    assert DiarizationSettings.from_env_and_params({"min_duration_on": -3.0}).min_duration_on == 0.0
    assert DiarizationSettings.from_env_and_params({"min_duration_off": 60.0}).min_duration_off == 5.0
    assert DiarizationSettings.from_env_and_params({"merge_gap": 90.0}).merge_gap == 5.0
    assert DiarizationSettings.from_env_and_params({"merge_similarity": 4.0}).merge_similarity == 1.0
    assert DiarizationSettings.from_env_and_params({"min_speaker_seconds": -1.0}).min_speaker_seconds == 0.0


def test_speaker_count_is_automatic_unless_it_is_a_plausible_head_count():
    assert DiarizationSettings.from_env_and_params({"num_speakers": 0}).num_speakers == -1
    assert DiarizationSettings.from_env_and_params({"num_speakers": -5}).num_speakers == -1
    assert DiarizationSettings.from_env_and_params({"num_speakers": 99}).num_speakers == 20
    assert DiarizationSettings.from_env_and_params({"num_speakers": 4.9}).num_speakers == 4


@pytest.mark.parametrize("raw", ["", "loud", None, "nan", "inf"])
def test_unusable_values_fall_back_to_the_default(raw):
    settings = DiarizationSettings.from_env_and_params({"threshold": raw})
    assert settings.threshold == 0.5


def test_a_non_mapping_settings_payload_is_ignored():
    assert DiarizationSettings.from_env_and_params("0.8").threshold == 0.5
    assert DiarizationSettings.from_env_and_params({"threshold": 0.8}, {"LOCUS_DIARIZATION_THRESHOLD": "x"}).threshold == 0.8


def test_table_defaults_never_raise_for_extreme_input():
    extreme = DiarizationSettings.from_env_and_params(
        {"threshold": 1e9, "num_speakers": 1e9, "min_duration_on": 1e9, "min_speaker_seconds": 1e9}
    )
    assert 0.1 <= extreme.threshold <= 1.5
    assert extreme.num_speakers == 20
    assert extreme.min_duration_on == 5.0
    assert extreme.min_speaker_seconds == 7200.0
