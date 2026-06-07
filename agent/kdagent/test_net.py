"""Network tests: head shapes, valid prior/value distributions, and end-to-end gradients.
Run: `../.venv/Scripts/python -m kdagent.test_net` from the agent/ dir (or with pytest)."""
import random

import numpy as np
import torch

import kingdomino as kd
from kdagent.encoder import encode
from kdagent.net import KingdominoNet


def _player_states(seed, n):
    """Collect a few encoded player-node states from a random game."""
    rng = random.Random(seed)
    g = kd.Game(seed, 2)
    out = []
    while len(out) < n:
        if g.is_terminal():
            g = kd.Game(seed + 1000 + len(out), 2)
            continue
        if g.is_chance():
            g.apply_chance()
            continue
        out.append(encode(g))
        g.apply(rng.randrange(g.num_actions()))
    return out


def test_forward_shapes_and_distributions():
    torch.manual_seed(0)
    net = KingdominoNet(player_count=2).eval()
    assert net.num_params() > 0
    for enc in _player_states(1, 6):
        with torch.no_grad():
            logits, value = net.policy_value(enc)
        a = len(enc.actions.type_id)
        assert logits.shape == (a,) and torch.isfinite(logits).all()
        assert value.shape == (2,) and torch.isfinite(value).all()
        priors = torch.softmax(logits, dim=-1)
        assert abs(float(priors.sum()) - 1.0) < 1e-5
        val = torch.softmax(value, dim=-1)  # seat-relative outcome distribution (sums to 1)
        assert abs(float(val.sum()) - 1.0) < 1e-5
    print("  forward shapes + valid prior/value distributions OK")


def test_backward_trains():
    torch.manual_seed(0)
    net = KingdominoNet(player_count=2)
    opt = torch.optim.Adam(net.parameters(), lr=1e-3)
    states = _player_states(2, 8)
    # One optimization step against arbitrary targets just to prove the whole graph is
    # differentiable (uniform policy target, a fixed value target).
    opt.zero_grad()
    loss = torch.zeros(())
    for enc in states:
        logits, value = net.policy_value(enc)
        a = logits.shape[0]
        logp = torch.log_softmax(logits, dim=-1)
        loss = loss - logp.mean()  # push toward uniform
        vt = torch.tensor([1.0, 0.0])
        loss = loss + torch.nn.functional.mse_loss(torch.softmax(value, -1), vt)
    loss.backward()
    grads = [p.grad for p in net.parameters() if p.grad is not None]
    assert grads, "no gradients flowed"
    assert any(float(g.abs().sum()) > 0 for g in grads), "all-zero gradients"
    opt.step()
    print(f"  backward/optimizer step OK (loss={float(loss):.3f}, params={net.num_params()})")


def test_cuda_forward_if_available():
    if not torch.cuda.is_available():
        print("  cuda: not available, skipped")
        return
    net = KingdominoNet(player_count=2).to("cuda").eval()
    enc = _player_states(3, 1)[0]
    with torch.no_grad():
        logits, value = net.policy_value(enc, device="cuda")
    assert logits.is_cuda and value.is_cuda and torch.isfinite(logits).all()
    print("  cuda forward OK")


if __name__ == "__main__":
    print("kdagent net tests")
    test_forward_shapes_and_distributions()
    test_backward_trains()
    test_cuda_forward_if_available()
    print("ALL OK")
