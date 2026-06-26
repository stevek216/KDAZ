"""Trainer tests: collation shapes/targets, that a batch overfits (loss drops), and a
checkpoint round-trips through load_net. Run: `../.venv/Scripts/python -m kdagent.test_train`."""
import json
import tempfile

import numpy as np
import torch

import kingdomino as kd
from kdagent.dataset import collate
from kdagent.net import KingdominoNet, load_net
from kdagent.train import losses


def make_corpus(n_games=6, sims=16, seed=0):
    lines = kd.selfplay_batch(n_games=n_games, players=2, n_sims=sims, seed=seed)
    return [json.loads(l) for l in lines]


def test_collate_shapes_and_targets():
    recs = make_corpus()
    batch = collate(recs[:32], pc=2)
    b = len(batch)
    assert batch.board.shape == (b, 2 * 13, 13, 13), batch.board.shape
    assert batch.a_type.shape == batch.policy.shape == batch.a_mask.shape
    assert batch.value_rel.shape == (b, 2)
    # policy is a distribution over the real actions of each row; value_rel sums to 1.
    psum = batch.policy.sum(dim=1).numpy()
    assert np.allclose(psum, 1.0, atol=1e-4), psum[:5]
    assert np.allclose(batch.value_rel.sum(dim=1).numpy(), 1.0, atol=1e-4)
    # pad slots are masked and carry no policy mass.
    assert torch.all(batch.policy[~batch.a_mask] == 0)
    print(f"  collate: batch of {b}, shapes + targets OK")


def test_overfit_one_batch():
    torch.manual_seed(0)
    recs = make_corpus(n_games=8, sims=16)
    batch = collate(recs[:64], pc=2)
    net = KingdominoNet(player_count=2, ch=16, board_blocks=2)
    opt = torch.optim.Adam(net.parameters(), lr=2e-3)
    init = float(losses(net, batch, value_coef=1.0)[0])
    for _ in range(80):
        opt.zero_grad()
        loss, _, _, _ = losses(net, batch, value_coef=1.0)
        loss.backward()
        opt.step()
    final = float(losses(net, batch, value_coef=1.0)[0])
    assert final < 0.85 * init, f"overfit should reduce loss: {init:.3f} -> {final:.3f}"
    print(f"  overfit: loss {init:.3f} -> {final:.3f} OK")


def test_checkpoint_roundtrip():
    torch.manual_seed(0)
    cfg = {"player_count": 2, "ch": 16, "board_blocks": 2}
    net = KingdominoNet(**cfg).eval()
    recs = make_corpus(n_games=2, sims=8)
    # encode one sample to compare net outputs before/after a save+load.
    from kdagent.encoder import encode_obs

    enc = encode_obs(recs[0]["obs"], recs[0]["legal"])
    with torch.no_grad():
        l0, v0 = net.policy_value(enc)
    with tempfile.NamedTemporaryFile(suffix=".pt", delete=False) as tf:
        path = tf.name
    torch.save({"model": net.state_dict(), "net_cfg": cfg, "epoch": 1, "val": 0.0}, path)
    loaded, ck = load_net(path, device="cpu")
    assert ck["net_cfg"] == cfg
    with torch.no_grad():
        l1, v1 = loaded.policy_value(enc)
    assert torch.allclose(l0, l1) and torch.allclose(v0, v1), "reloaded net must match"
    print("  checkpoint round-trip through load_net OK")


if __name__ == "__main__":
    print("kdagent trainer tests")
    test_collate_shapes_and_targets()
    test_overfit_one_batch()
    test_checkpoint_roundtrip()
    print("ALL OK")
